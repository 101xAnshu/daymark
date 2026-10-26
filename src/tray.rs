//! Optional StatusNotifierItem tray.
//!
//! The tray is a thin menu over the existing local socket. It deliberately
//! owns no Slint types: the UI thread stays untouched, and every action is
//! delivered as the same command the application already understands. When no
//! StatusNotifierWatcher (or no session bus) exists, the tray thread logs once
//! and leaves the application usable.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Category, Icon, MenuItem, Status, ToolTip};

use daymark::date;
use daymark::instance;
use daymark::logging;
use daymark::settings::Settings;
use daymark::tracker::TrackerHandle;

/// Icons are small because the host renders them at panel size anyway.
const ICON_SIZE: i32 = 24;
/// Low-frequency settings poll. A pause request is applied immediately by the
/// UI tick; this poll only refreshes the icon after a timer expires or another
/// path changes the pause state.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The light-theme accent from the interface design tokens.
const ACCENT_RGB: [u8; 3] = [0xaa, 0x54, 0x31];
/// A quieter neutral for the paused variant, so it does not read as an alarm.
const PAUSED_RGB: [u8; 3] = [0x6b, 0x62, 0x58];

/// Pause durations offered by the tray menu. Zero means "until resumed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PausePreset {
    FifteenMinutes,
    OneHour,
    UntilResumed,
}

impl PausePreset {
    pub fn seconds(self) -> i64 {
        match self {
            Self::FifteenMinutes => 15 * 60,
            Self::OneHour => 60 * 60,
            Self::UntilResumed => 0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::FifteenMinutes => "Pause 15 minutes",
            Self::OneHour => "Pause 1 hour",
            Self::UntilResumed => "Pause until resumed",
        }
    }
}

/// Every item the menu can contain. Kept separate from the ksni menu so the
/// visibility order can be tested without a session bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Pause(PausePreset),
    Resume,
    Quit,
}

impl TrayAction {
    fn label(self) -> &'static str {
        match self {
            Self::Open => "Open Daymark",
            Self::Pause(preset) => preset.label(),
            Self::Resume => "Resume",
            Self::Quit => "Quit Daymark",
        }
    }

    fn request(self) -> TrayRequest {
        match self {
            Self::Open => TrayRequest::Show,
            Self::Pause(preset) => TrayRequest::Pause(preset.seconds()),
            Self::Resume => TrayRequest::Resume,
            Self::Quit => TrayRequest::Quit,
        }
    }

    fn send(self) -> bool {
        self.request().send()
    }
}

/// The socket command an action turns into. This is the pure mapping that the
/// tests pin down; the instance helpers own the actual socket write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayRequest {
    Show,
    Pause(i64),
    Resume,
    Quit,
}

impl TrayRequest {
    fn send(self) -> bool {
        match self {
            Self::Show => instance::request_show(),
            Self::Pause(seconds) => instance::request_pause(seconds),
            Self::Resume => instance::request_resume(),
            Self::Quit => instance::request_quit(),
        }
    }
}

/// The actions in menu order. Resume appears only while a pause is active.
pub fn menu_actions(paused: bool) -> Vec<TrayAction> {
    let mut actions = vec![
        TrayAction::Open,
        TrayAction::Pause(PausePreset::FifteenMinutes),
        TrayAction::Pause(PausePreset::OneHour),
        TrayAction::Pause(PausePreset::UntilResumed),
    ];
    if paused {
        actions.push(TrayAction::Resume);
    }
    actions.push(TrayAction::Quit);
    actions
}

#[cfg(test)]
pub fn menu_labels(paused: bool) -> Vec<&'static str> {
    menu_actions(paused)
        .into_iter()
        .map(TrayAction::label)
        .collect()
}

/// The subset of settings the tray needs, resolved at a point in time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrayState {
    pub paused: bool,
    pub paused_until: i64,
}

impl TrayState {
    /// A timed pause whose deadline has passed is already resumed, even before
    /// the tracker gets a chance to write the cleared state back.
    pub fn from_settings(settings: &Settings, now: i64) -> Self {
        let paused = settings.paused && (settings.paused_until == 0 || settings.paused_until > now);
        if paused {
            Self {
                paused: true,
                paused_until: settings.paused_until,
            }
        } else {
            Self::default()
        }
    }

    fn remaining_minutes(self, now: i64) -> i64 {
        if self.paused && self.paused_until > now {
            (self.paused_until - now + 59) / 60
        } else {
            0
        }
    }
}

/// Tooltip text for the current state. Kept pure so it can be tested without
/// a tray host.
pub fn tooltip_text(state: TrayState, now: i64) -> String {
    if !state.paused {
        return "Tracking is on.".to_string();
    }
    if state.paused_until > now {
        format!(
            "Tracking is paused — {} remaining.",
            date::format_duration(state.paused_until - now)
        )
    } else {
        "Tracking is paused until you resume.".to_string()
    }
}

/// Generate a small ARGB32 icon. The running icon uses the interface accent
/// with a clock glyph; the paused icon uses a neutral background and two pause
/// bars. ksni expects bytes in network order: alpha, red, green, blue.
pub fn icon_for(paused: bool) -> Icon {
    let background = if paused { PAUSED_RGB } else { ACCENT_RGB };
    let mut data = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let center = (ICON_SIZE as f32 - 1.0) / 2.0;
    let radius = ICON_SIZE as f32 * 0.46;
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let mut pixel = [0u8; 4];
            if distance <= radius {
                pixel = [255, background[0], background[1], background[2]];
                if paused {
                    let left_bar = (8..=10).contains(&x) && (7..=16).contains(&y);
                    let right_bar = (13..=15).contains(&x) && (7..=16).contains(&y);
                    if left_bar || right_bar {
                        pixel = [255, 255, 255, 255];
                    }
                } else {
                    let ring = (distance - 6.0).abs() <= 0.9;
                    let minute_hand = (11..=12).contains(&x) && (6..=11).contains(&y);
                    let hour_hand = (12..=16).contains(&x) && (11..=12).contains(&y);
                    if ring || minute_hand || hour_hand {
                        pixel = [255, 255, 255, 255];
                    }
                }
            }
            let offset = ((y * ICON_SIZE + x) * 4) as usize;
            data[offset..offset + 4].copy_from_slice(&pixel);
        }
    }
    Icon {
        width: ICON_SIZE,
        height: ICON_SIZE,
        data,
    }
}

struct DaymarkTray {
    state: TrayState,
    running_icon: Icon,
    paused_icon: Icon,
    offline_logged: AtomicBool,
}

impl DaymarkTray {
    fn new(state: TrayState) -> Self {
        Self {
            state,
            running_icon: icon_for(false),
            paused_icon: icon_for(true),
            offline_logged: AtomicBool::new(false),
        }
    }

    fn set_state(&mut self, state: TrayState) {
        self.state = state;
    }

    fn icon(&self) -> &Icon {
        if self.state.paused {
            &self.paused_icon
        } else {
            &self.running_icon
        }
    }

    /// Apply the user's choice locally so the icon reacts before the next
    /// poll. The poll replaces this with the tracker's authoritative state.
    fn note_action(&mut self, action: TrayAction) {
        match action {
            TrayAction::Pause(preset) => {
                let seconds = preset.seconds();
                self.state = TrayState {
                    paused: true,
                    paused_until: if seconds > 0 {
                        date::unix_now() + seconds
                    } else {
                        0
                    },
                };
            }
            TrayAction::Resume => self.state = TrayState::default(),
            TrayAction::Open | TrayAction::Quit => {}
        }
    }
}

fn action_item(action: TrayAction) -> MenuItem<DaymarkTray> {
    StandardItem {
        label: action.label().to_string(),
        activate: Box::new(move |tray: &mut DaymarkTray| {
            if action.send() {
                tray.note_action(action);
            }
        }),
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for DaymarkTray {
    fn id(&self) -> String {
        "daymark".to_string()
    }

    fn title(&self) -> String {
        "Daymark".to_string()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn status(&self) -> Status {
        Status::Active
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![self.icon().clone()]
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_name: String::new(),
            icon_pixmap: vec![self.icon().clone()],
            title: "Daymark".to_string(),
            description: tooltip_text(self.state, date::unix_now()),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = instance::request_show();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items = vec![action_item(TrayAction::Open), MenuItem::Separator];
        for action in menu_actions(self.state.paused) {
            match action {
                TrayAction::Open => {}
                TrayAction::Resume => {
                    items.push(MenuItem::Separator);
                    items.push(action_item(action));
                }
                TrayAction::Quit => {
                    items.push(MenuItem::Separator);
                    items.push(action_item(action));
                }
                TrayAction::Pause(_) => items.push(action_item(action)),
            }
        }
        items
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        if !self.offline_logged.swap(true, Ordering::Relaxed) {
            logging::info(
                "tray",
                &format!("waiting for a StatusNotifierWatcher ({reason:?})"),
            );
        }
        // Keep the service alive: the watcher may come back when the panel
        // restarts, and the tray then registers itself again.
        true
    }

    fn watcher_online(&self) {
        logging::info("tray", "StatusNotifierWatcher available");
    }
}

/// Guard for the tray helper thread. Dropping it only asks the thread to stop;
/// the process exits right after the event loop, so it deliberately does not
/// join a host call that might still be in flight.
pub struct TrayHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TrayHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

/// Start the tray once. Returns `None` when the tray was already started or
/// the helper thread could not be created; both are non-fatal.
pub fn spawn(tracker: Option<TrackerHandle>) -> Option<TrayHandle> {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return None;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    match std::thread::Builder::new()
        .name("daymark-tray".to_string())
        .spawn(move || run(tracker, thread_stop))
    {
        Ok(thread) => Some(TrayHandle {
            stop,
            thread: Some(thread),
        }),
        Err(error) => {
            logging::error("tray", &format!("could not start the tray thread: {error}"));
            None
        }
    }
}

fn run(tracker: Option<TrackerHandle>, stop: Arc<AtomicBool>) {
    let now = date::unix_now();
    let initial = tracker
        .as_ref()
        .map(|tracker| TrayState::from_settings(&tracker.settings(), now))
        .unwrap_or_default();
    // `assume_sni_available` turns a missing watcher into `watcher_offline`
    // instead of a failed spawn, so a panel started later can still pick the
    // indicator up. A missing session bus still fails here.
    let handle = match DaymarkTray::new(initial).assume_sni_available(true).spawn() {
        Ok(handle) => handle,
        Err(error) => {
            logging::info("tray", &format!("not available: {error}"));
            return;
        }
    };
    let mut last = (initial, initial.remaining_minutes(now));
    while !stop.load(Ordering::Relaxed) {
        std::thread::park_timeout(POLL_INTERVAL);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let Some(tracker) = &tracker else {
            continue;
        };
        let now = date::unix_now();
        let state = TrayState::from_settings(&tracker.settings(), now);
        let key = (state, state.remaining_minutes(now));
        if key == last {
            continue;
        }
        last = key;
        if handle.update(move |tray| tray.set_state(state)).is_none() {
            break;
        }
    }
    handle.shutdown();
}

