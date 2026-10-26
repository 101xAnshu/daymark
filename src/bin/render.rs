//! Headless screenshot renderer used for visual QA. Build with the normal
//! release profile, but it is not installed with the application.

use std::path::PathBuf;
use std::rc::Rc;

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize};

use daymark::app::AppController;
use daymark::date::Day;
use daymark::db::Store;
use daymark::desktop::ResolvedApp;
use daymark::settings::{Settings, ThemePref};
use daymark::tracker::{TrackerHandle, TrackingStatus};
use daymark::{AppWindow, Theme};

/// One quiet audio-backed session on today's timeline, far enough from the
/// demo blocks that the note is easy to inspect.
fn seed_presence(store: &Store) {
    let today = Day::today();
    let app = ResolvedApp {
        key: "org.example.Player".into(),
        name: "Media Player".into(),
        icon: None,
        desktop_id: None,
        wm_class: None,
        matched: false,
    };
    if let Ok(app_id) = store.upsert_app(&app, today.start_ts()) {
        if let Ok(id) =
            store.open_session_with_presence(app_id, today.start_ts() + 22 * 3600, "audio")
        {
            let _ = store.close_session(id, today.start_ts() + 22 * 3600 + 12 * 60);
        }
    }
}

struct HeadlessPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = PathBuf::from("/tmp/opencode/daymark.png");
    let mut state = "populated".to_string();
    let mut width = 1120u32;
    let mut height = 780u32;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = PathBuf::from(args.get(i).ok_or("missing --out value")?);
            }
            "--state" => {
                i += 1;
                state = args.get(i).ok_or("missing --state value")?.clone();
            }
            "--width" => {
                i += 1;
                width = args.get(i).ok_or("missing width")?.parse()?;
            }
            "--height" => {
                i += 1;
                height = args.get(i).ok_or("missing height")?.parse()?;
            }
            other => return Err(format!("unknown argument {other}").into()),
        }
        i += 1;
    }

    let db_path = std::env::temp_dir().join(format!(
        "daymark-render-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let store = Store::open_at(&db_path)?;
    if state != "empty"
        && state != "light-empty"
        && state != "empty-banner"
        && state != "empty-waiting"
        && state != "rhythm-empty"
    {
        daymark::demo::seed(&store);
        if state == "presence" {
            seed_presence(&store);
        }
        if state == "totals" || state == "history-rollup" {
            // Simulate retention: older days keep their daily totals only.
            let _ = store.prune_before(Day::today().add_days(-5).start_ts());
        }
    }
    let mut settings = Settings {
        autostart: true,
        ..Settings::default()
    };
    let status = TrackingStatus::Active;
    let (handle_status, detail) =
        if state == "setup" || state == "banner" || state == "empty-banner" {
            (
                TrackingStatus::NeedsSetup,
                "GNOME integration is not installed",
            )
        } else if state == "waiting" || state == "empty-waiting" {
            (
                TrackingStatus::Waiting,
                "Sign out and back in once to activate tracking",
            )
        } else {
            (status, "Tracking Brave Web Browser")
        };
    if state == "dark" || state == "dark-settings" {
        settings.theme = ThemePref::Dark;
    }
    if state == "light"
        || state == "light-settings"
        || state == "light-empty"
        || state == "light-rhythm"
    {
        settings.theme = ThemePref::Light;
    }
    let tracker = TrackerHandle::demo(settings.clone(), handle_status, detail);
    if state == "now" {
        // A live session lets the screenshot exercises the Now metric without
        // touching the seeded database.
        let now = daymark::date::unix_now();
        if let Ok(mut live) = tracker.live.lock() {
            live.current = Some(daymark::tracker::LiveCurrent {
                app_id: 1,
                key: "brave-browser.desktop".into(),
                name: "Brave Web Browser".into(),
                icon: Some("brave-browser".into()),
                started: now - 23 * 60,
                elapsed: 23 * 60,
                presence: daymark::tracker::Presence::Input,
            });
        }
    }

    let scale = 1.0f32;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(
        (width as f32 * scale) as u32,
        (height as f32 * scale) as u32,
    ));
    slint::platform::set_platform(Box::new(HeadlessPlatform {
        window: window.clone(),
    }))
    .ok();

    let ui = AppWindow::new()?;
    let controller = AppController::new(ui, Some(store), Some(tracker), settings, None);
    {
        let mut ctl = controller.borrow_mut();
        match state.as_str() {
            "history" => ctl.set_view_for_render(1),
            "rhythm" => ctl.set_view_for_render(2),
            "light-rhythm" => ctl.set_view_for_render(2),
            "rhythm-empty" => ctl.set_view_for_render(2),
            "timeline" => {}
            "timeline-empty" => ctl.set_selected_for_render(-9),
            "now" => {}
            "settings" | "dark-settings" | "light-settings" => ctl.open_settings_for_render(),
            "update" => {
                ctl.open_settings_for_render();
                ctl.ui.set_update_notice(
                    "Daymark 0.1.2 is available. Update with your package manager.".into(),
                );
            }
            "delete" => {
                ctl.open_settings_for_render();
                ctl.ui.set_confirm_delete(true);
            }
            "settings-error" => {
                ctl.open_settings_for_render();
                ctl.set_external_error("Could not open the data folder.".into());
            }
            "maximized" => ctl.ui.set_maximized(true),
            "setup" => ctl.open_setup_for_render(false),
            "setup-ready" => ctl.open_setup_for_render(true),
            "banner" => { /* the setup banner is shown automatically */ }
            "past" => ctl.set_selected_for_render(-3),
            "past-empty" => ctl.set_selected_for_render(-5),
            "totals" => ctl.set_selected_for_render(-6),
            "history-rollup" => ctl.set_view_for_render(1),
            "error" => ctl.set_load_error(
                "The local data file could not be opened: permission denied.".into(),
            ),
            "light-empty" | "empty-banner" | "empty-waiting" => {}
            _ => {}
        }
    }
    {
        let ctl = controller.borrow();
        eprintln!(
            "render state={} banner={} code={} action={}",
            state,
            ctl.ui.get_show_banner(),
            ctl.ui.get_tracking_code(),
            ctl.ui.get_banner_action()
        );
        if state == "dark" || state == "dark-settings" {
            ctl.ui.global::<Theme>().set_dark(true);
        }
        ctl.ui.show()?;
    }
    if state == "search" {
        let pos = slint::LogicalPosition::new(550.0, 426.0);
        window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
            position: pos,
            button: slint::platform::PointerEventButton::Left,
        });
        window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
            position: pos,
            button: slint::platform::PointerEventButton::Left,
        });
    }
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    slint::platform::update_timers_and_animations();

    let buffer = {
        let ctl = controller.borrow();
        ctl.ui.window().take_snapshot()?
    };
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    {
        let file = std::io::BufWriter::new(std::fs::File::create(&out)?);
        let mut encoder = png::Encoder::new(file, buffer.width(), buffer.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        let mut bytes = Vec::with_capacity(buffer.as_slice().len() * 4);
        for pixel in buffer.as_slice() {
            // The software renderer's snapshot is RGBx: the fourth byte is
            // padding, not an alpha channel. Writing it verbatim produced fully
            // transparent PNGs (they looked like a blank sheet everywhere).
            bytes.extend_from_slice(&[pixel.r, pixel.g, pixel.b, 255]);
        }
        writer.write_image_data(&bytes)?;
    }
    drop(controller);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", db_path.display()));
    }
    println!(
        "wrote {} ({}×{})",
        out.display(),
        buffer.width(),
        buffer.height()
    );
    Ok(())
}
