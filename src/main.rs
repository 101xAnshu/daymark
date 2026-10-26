use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use slint::ComponentHandle;

use daymark::AppWindow;
use daymark::app::AppController;
use daymark::db::{self, Store};
use daymark::diagnostics;
use daymark::instance::{self, COMMAND_NONE, COMMAND_QUIT, InstanceGuard};
use daymark::logging;
use daymark::settings::{self, Settings};
use daymark::tracker;

/// Refresh interval while the window is visible.
const UI_TICK: Duration = Duration::from_millis(700);

thread_local! {
    /// The lifecycle timer lives on the UI thread. The IPC helper posts a
    /// closure that reaches it after a hidden window has stopped it.
    static UI_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

fn main() -> std::process::ExitCode {
    match real_main() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            logging::error("run", &e);
            eprintln!("daymark: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<(), String> {
    let mut background = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--background" | "--autostart" | "--hidden" => background = true,
            "--quit" => {
                if instance::request_quit() {
                    logging::info("quit_requested", "sent");
                } else {
                    logging::error("quit_requested", "no running instance");
                    eprintln!("daymark: no running instance");
                }
                return Ok(());
            }
            "--setup-tracking" => {
                let caps = daymark::platform::install_extension()?;
                logging::info(
                    "extension_install",
                    &format!(
                        "enabled={} state_fresh={}",
                        caps.extension_enabled, caps.state_fresh
                    ),
                );
                println!(
                    "Daymark tracking extension installed (enabled: {}).",
                    caps.extension_enabled
                );
                if !caps.state_fresh {
                    println!("Sign out and back in once so GNOME can load the new extension.");
                }
                return Ok(());
            }
            "--version" | "-V" => {
                println!("Daymark {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--status" => {
                println!("{}", diagnostics::status_output());
                return Ok(());
            }
            "doctor" => {
                print!("{}", diagnostics::doctor_output());
                return Ok(());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => {
                return Err(format!("unknown option “{other}”. Try `daymark --help`."));
            }
        }
    }

    let guard = match InstanceGuard::acquire()? {
        Some(guard) => guard,
        None => return Ok(()),
    };

    logging::init();
    let started = Instant::now();
    logging::info(
        "started",
        &format!(
            "version={} background={}",
            env!("CARGO_PKG_VERSION"),
            background
        ),
    );

    // The interface reads through its own connection so the tracker never
    // blocks a chart update.
    let (ui_store, mut settings, store_error) = match Store::open_default() {
        Ok(store) => {
            let settings = Settings::load(&store);
            (Some(store), settings, None)
        }
        Err(e) => {
            logging::error("database", &e);
            (None, Settings::default(), Some(e))
        }
    };
    if let Some(store) = &ui_store {
        logging::info("database", &format!("sessions={}", store.session_count()));
    }

    let autostart_error =
        daymark::platform::apply_autostart_respecting_disabled(settings.autostart).err();
    if let Some(error) = &autostart_error {
        logging::error("autostart", error);
    }
    if autostart_error.is_some() {
        settings.autostart = daymark::platform::autostart_installed();
    }
    // An entry disabled with X-GNOME-Autostart-enabled=false or Hidden=true
    // is the user's choice: keep it and mirror the effective state in the
    // interface instead of quietly switching it back on at every launch.
    if settings.autostart && daymark::platform::autostart_disabled() {
        settings.autostart = false;
    }
    if let Err(error) = std::fs::create_dir_all(db::runtime_dir()) {
        logging::error("runtime_dir", &error.to_string());
    }
    settings::write_extension_config(&db::runtime_dir(), settings.idle_seconds);

    // One-time cleanup: fold old fallback rows (wmclass:…) into their desktop
    // entries and drop shell-internal identities such as gjs.
    if let Some(store) = &ui_store {
        daymark::db::migrate_legacy_identities(store);
    }

    let tracker = if ui_store.is_some() {
        match Store::open_default() {
            Ok(write_store) => Some(tracker::spawn(write_store, settings.clone())),
            Err(e) => {
                logging::error("tracking_database", &e);
                eprintln!("daymark: tracking database unavailable: {e}");
                None
            }
        }
    } else {
        None
    };

    let command = Arc::new(AtomicU8::new(COMMAND_NONE));
    let shutting_down = Arc::new(AtomicBool::new(false));
    let waker: Arc<std::sync::Mutex<Option<std::thread::Thread>>> =
        Arc::new(std::sync::Mutex::new(None));
    // The listener only stores an atomic command. In the headless process the
    // parked main thread is the waker; once a window exists a small helper
    // thread forwards IPC wakeups into the Slint event loop instead.
    guard.listen(shutting_down.clone(), command.clone(), Some(waker.clone()))?;

    // `--background` must be genuinely invisible: no Slint platform, no winit
    // window, no event loop. The tracker and IPC listener run on their own
    // threads. The interface is created only when the user asks for it, either
    // from the application grid (a second launch sends "show") or by launching
    // Daymark normally.
    if background {
        if let Ok(mut slot) = waker.lock() {
            *slot = Some(std::thread::current());
        }
        loop {
            match command.swap(COMMAND_NONE, Ordering::SeqCst) {
                instance::COMMAND_SHOW => break,
                COMMAND_QUIT => {
                    shutting_down.store(true, Ordering::Relaxed);
                    if let Some(tracker) = &tracker {
                        tracker.shutdown();
                    }
                    logging::info(
                        "stopped",
                        &format!("uptime_s={}", started.elapsed().as_secs()),
                    );
                    drop(guard);
                    return Ok(());
                }
                _ => std::thread::park(),
            }
        }
    }

    let ui = AppWindow::new().map_err(|e| {
        format!(
            "could not open a window ({e}). Daymark needs a running GNOME Wayland or X11 session."
        )
    })?;

    let controller = AppController::new(ui, ui_store, tracker, settings, Some(command.clone()));
    if let Some(e) = store_error {
        controller.borrow_mut().set_load_error(e);
    }
    if let Some(e) = autostart_error {
        controller
            .borrow_mut()
            .set_external_error(format!("Could not configure startup: {e}"));
    }
    // First launch after install: open the setup sheet once, explain the
    // one re-login and offer Start at login. The dashboard behind the sheet
    // stays fully usable.
    controller.borrow_mut().begin_first_run();

    // A single, inexpensive lifecycle timer. It handles the local IPC socket
    // and refreshes only when visible data actually changed. While the window
    // is hidden the timer stops; the IPC listener unparks a helper thread that
    // posts a wakeup into the event loop, so reopening from the application grid
    // still works without a periodic poll.
    let timer = slint::Timer::default();
    let timer_controller = controller.clone();
    timer.start(slint::TimerMode::Repeated, UI_TICK, move || {
        let hidden = match timer_controller.try_borrow_mut() {
            Ok(mut ctl) => {
                ctl.tick();
                ctl.window_hidden()
            }
            // A callback owns the controller right now; keep the timer alive.
            Err(_) => false,
        };
        UI_TIMER.with(|slot| {
            if let Ok(slot) = slot.try_borrow() {
                if let Some(timer) = slot.as_ref() {
                    if hidden {
                        timer.stop();
                    } else if timer.interval() != UI_TICK {
                        timer.set_interval(UI_TICK);
                    }
                }
            }
        });
    });
    // Keep the timer for the life of the event loop; the callback above and
    // the IPC waker both look it up on the UI thread.
    UI_TIMER.with(|slot| *slot.borrow_mut() = Some(timer));

    let waker_thread = std::thread::Builder::new()
        .name("daymark-ui-wake".into())
        .spawn(move || {
            loop {
                std::thread::park();
                if slint::invoke_from_event_loop(wake_ui_timer).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| format!("could not start the interface waker: {e}"))?;
    if let Ok(mut slot) = waker.lock() {
        *slot = Some(waker_thread.thread().clone());
    }
    drop(waker_thread);

    controller
        .borrow()
        .ui
        .show()
        .map_err(|e| format!("could not show the window: {e}"))?;

    slint::run_event_loop().map_err(|e| format!("the interface stopped: {e}"))?;

    shutting_down.store(true, Ordering::Relaxed);
    if let Some(tracker) = controller.borrow().tracker_handle() {
        tracker.shutdown();
    }
    logging::info(
        "stopped",
        &format!("uptime_s={}", started.elapsed().as_secs()),
    );
    drop(guard);
    Ok(())
}

/// Runs on the UI thread. A hidden window has stopped the regular timer; the
/// IPC listener unparks its helper thread, which calls this through
/// `slint::invoke_from_event_loop` so show and quit are handled immediately.
fn wake_ui_timer() {
    UI_TIMER.with(|slot| {
        if let Ok(slot) = slot.try_borrow() {
            if let Some(timer) = slot.as_ref() {
                timer.set_interval(Duration::from_millis(1));
                timer.restart();
            }
        }
    });
}

fn print_help() {
    println!(
        "Daymark {}\n\
         A quiet screen time journal for GNOME.\n\n\
         Usage: daymark [options]\n\
                  daymark doctor\n\n\
         Options:\n\
           --background     start hidden and keep tracking (used at login)\n\
           --quit           stop the running instance\n\
           --setup-tracking install the GNOME companion extension\n\
           --status         print one line of tracking state for scripts\n\
           --version        print the version\n\
           -h, --help       show this help\n\n\
         Commands:\n\
           doctor           read-only database, companion and log check\n\n\
         The window can be reopened from the application grid. Daymark keeps\n\
         tracking while hidden; quit it from Settings.",
        env!("CARGO_PKG_VERSION")
    );
}
