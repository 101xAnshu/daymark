use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use slint::ComponentHandle;

use daymark::AppWindow;
use daymark::app::AppController;
use daymark::db::{self, Store};
use daymark::diagnostics;
use daymark::instance::{self, COMMAND_NONE, COMMAND_QUIT, InstanceGuard};
use daymark::settings::{self, Settings};
use daymark::tracker;

fn main() -> std::process::ExitCode {
    match real_main() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
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
                if !instance::request_quit() {
                    eprintln!("daymark: no running instance");
                }
                return Ok(());
            }
            "--setup-tracking" => {
                let caps = daymark::platform::install_extension()?;
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

    // The interface reads through its own connection so the tracker never
    // blocks a chart update.
    let (ui_store, mut settings, store_error) = match Store::open_default() {
        Ok(store) => {
            let settings = Settings::load(&store);
            (Some(store), settings, None)
        }
        Err(e) => (None, Settings::default(), Some(e)),
    };

    let autostart_error = daymark::platform::apply_autostart(settings.autostart).err();
    if autostart_error.is_some() {
        settings.autostart = daymark::platform::autostart_installed();
    }
    let _ = std::fs::create_dir_all(db::runtime_dir());
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
    guard.listen(
        shutting_down.clone(),
        command.clone(),
        if background {
            Some(waker.clone())
        } else {
            None
        },
    )?;

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

    // A single, inexpensive lifecycle timer. It handles the local IPC socket
    // and refreshes only when visible data actually changed.
    let timer = slint::Timer::default();
    let timer_controller = controller.clone();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(700),
        move || {
            if let Ok(mut ctl) = timer_controller.try_borrow_mut() {
                ctl.tick();
            }
        },
    );
    // The timer must outlive main's event loop.
    Box::leak(Box::new(timer));

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
    drop(guard);
    Ok(())
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
