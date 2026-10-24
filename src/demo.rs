//! Deterministic sample data for the screenshot renderer and visual QA. Never
//! used by the normal application.

use crate::date::Day;
use crate::db::Store;
use crate::desktop::ResolvedApp;

struct DemoApp {
    key: &'static str,
    name: &'static str,
    icon: &'static str,
}

const APPS: &[DemoApp] = &[
    DemoApp {
        key: "brave-browser.desktop",
        name: "Brave Web Browser",
        icon: "brave-browser",
    },
    DemoApp {
        key: "org.gnome.Ptyxis.desktop",
        name: "Terminal",
        icon: "org.gnome.Ptyxis",
    },
    DemoApp {
        key: "org.gnome.Nautilus.desktop",
        name: "Files",
        icon: "org.gnome.Nautilus",
    },
    DemoApp {
        key: "org.gnome.Loupe.desktop",
        name: "Image Viewer",
        icon: "org.gnome.Loupe",
    },
    DemoApp {
        key: "org.gnome.TextEditor.desktop",
        name: "Text Editor",
        icon: "org.gnome.TextEditor",
    },
    DemoApp {
        key: "org.gnome.Settings.desktop",
        name: "Settings",
        icon: "org.gnome.Settings",
    },
    DemoApp {
        key: "org.gnome.Calculator.desktop",
        name: "Calculator",
        icon: "org.gnome.Calculator",
    },
    DemoApp {
        key: "vlc.desktop",
        name: "VLC media player",
        icon: "vlc",
    },
];

// minute of day, app index, duration in minutes
const BLOCKS: &[(i64, usize, i64)] = &[
    (8 * 60 + 52, 1, 22),
    (9 * 60 + 14, 0, 48),
    (10 * 60 + 2, 2, 24),
    (10 * 60 + 30, 0, 62),
    (11 * 60 + 32, 4, 28),
    (12 * 60 + 2, 1, 27),
    (12 * 60 + 31, 3, 35),
    (13 * 60 + 10, 0, 64),
    (14 * 60 + 16, 5, 13),
    (14 * 60 + 34, 1, 31),
    (15 * 60 + 8, 6, 7),
    (15 * 60 + 18, 2, 26),
    (15 * 60 + 48, 7, 17),
];

pub fn seed(store: &Store) {
    let today = Day::today();
    let ids: Vec<i64> = APPS
        .iter()
        .map(|app| {
            let resolved = ResolvedApp {
                key: app.key.into(),
                name: app.name.into(),
                icon: Some(app.icon.into()),
                desktop_id: Some(app.key.into()),
                wm_class: None,
                matched: true,
            };
            store
                .upsert_app(&resolved, today.start_ts())
                .expect("demo app")
        })
        .collect();

    // A believable spread: a quiet weekend, a heavy Wednesday, an ordinary
    // week and a partial day.
    let factors = [
        0.92, 0.78, 1.08, 0.64, 0.0, 0.88, 0.71, 1.12, 0.83, 0.97, 0.55, 1.18, 0.9, 1.0,
    ];
    for (index, offset) in (-13..=0).enumerate() {
        let factor = factors[index];
        if factor <= 0.0 {
            continue;
        }
        let day = today.add_days(offset);
        let shift = (((offset * 7).rem_euclid(11) - 5) * 60) as i64;
        for (minute, app, duration) in BLOCKS {
            if *app == 7 && factor < 0.75 {
                continue;
            }
            let start = day.start_ts() + minute * 60 + shift;
            let end = start + ((*duration as f64 * factor) as i64) * 60;
            if let Ok(id) = store.open_session(ids[*app], start) {
                let _ = store.close_session(id, end);
            }
        }
    }
}
