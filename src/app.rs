//! UI controller: turns stored intervals into the small set of properties the
//! Slint dashboard consumes, and handles user actions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Image, ModelRc, SharedPixelBuffer, VecModel};

use crate::date::{self, Day};
use crate::db::{self, AppInfo, Store};
use crate::instance::{COMMAND_NONE, COMMAND_QUIT, COMMAND_SHOW};
use crate::logging;
use crate::settings::{self, Settings, ThemePref};
use crate::stats::{self, SessionRow};
use crate::tracker::{self, LiveState, TrackerHandle, TrackingStatus};
use crate::{AppRow, AppWindow, DayBar, HistoryRow, HourBar, Theme};

const HISTORY_DAYS: i32 = 14;
/// Decoded icons are kept small; the UI shows them in a 28 pt tile.
const ICON_MAX_PX: u32 = 96;
const ICON_CACHE_LIMIT: usize = 256;

struct IconCache {
    paths: HashMap<String, Option<PathBuf>>,
    images: HashMap<String, Option<Image>>,
}

impl IconCache {
    fn new() -> Self {
        Self {
            paths: HashMap::new(),
            images: HashMap::new(),
        }
    }

    fn get(&mut self, icon: &str) -> Option<Image> {
        if let Some(cached) = self.images.get(icon) {
            return cached.clone();
        }
        let path = self
            .paths
            .entry(icon.to_string())
            .or_insert_with(|| crate::desktop::find_icon(icon))
            .clone();
        let image = path.as_deref().and_then(decode_icon);
        if self.images.len() >= ICON_CACHE_LIMIT {
            self.images.clear();
        }
        self.images.insert(icon.to_string(), image.clone());
        image
    }
}

fn decode_icon(path: &std::path::Path) -> Option<Image> {
    let is_svg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    if is_svg {
        // Slint 1.8's `std` build already contains a vector renderer; the SVG
        // stays vector-backed and is rasterised at the size the tile needs.
        return Image::load_from_path(path).ok();
    }
    decode_png(path)
}

fn decode_png(path: &std::path::Path) -> Option<Image> {
    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    decode_png_reader(file)
}

fn decode_png_bytes(bytes: &[u8]) -> Option<Image> {
    decode_png_reader(std::io::BufReader::new(std::io::Cursor::new(bytes)))
}

fn decode_png_reader<R: std::io::BufRead + std::io::Seek>(reader: R) -> Option<Image> {
    let mut decoder = png::Decoder::new(reader);
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::normalize_to_color8(),
    );
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(&buf[..info.buffer_size()]),
        png::ColorType::Rgb => {
            for px in buf[..info.buffer_size()].chunks_exact(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in buf[..info.buffer_size()].chunks_exact(2) {
                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &px in &buf[..info.buffer_size()] {
                rgba.extend_from_slice(&[px, px, px, 255]);
            }
        }
        png::ColorType::Indexed => return None,
    }
    if rgba.len() != (w * h * 4) as usize {
        return None;
    }
    let (rgba, w, h) = downscale_rgba(rgba, w, h, ICON_MAX_PX);
    let buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, w, h);
    Some(Image::from_rgba8(buffer))
}

/// Box-filtered downscale so a 1024 px application icon does not occupy
/// megabytes for a 28 pt tile. Averaging alpha is fine for icon artwork.
fn downscale_rgba(rgba: Vec<u8>, w: u32, h: u32, max: u32) -> (Vec<u8>, u32, u32) {
    if w <= max && h <= max {
        return (rgba, w, h);
    }
    let scale = (max as f32 / w.max(h) as f32).min(1.0);
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        let sy0 = y * h / nh;
        let sy1 = (((y + 1) * h) / nh).max(sy0 + 1);
        for x in 0..nw {
            let sx0 = x * w / nw;
            let sx1 = (((x + 1) * w) / nw).max(sx0 + 1);
            let mut acc = [0u64; 4];
            let mut count = 0u64;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let i = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += rgba[i + c] as u64;
                    }
                    count += 1;
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / count.max(1)) as u8;
            }
        }
    }
    (out, nw, nh)
}

fn search_icon() -> Option<Image> {
    decode_png_bytes(include_bytes!("../assets/search.png"))
}

const PALETTE_LIGHT: [(u8, u8, u8); 8] = [
    (140, 90, 60),
    (95, 122, 92),
    (154, 106, 47),
    (92, 111, 138),
    (128, 96, 122),
    (79, 123, 120),
    (122, 100, 72),
    (94, 102, 110),
];
const PALETTE_DARK: [(u8, u8, u8); 8] = [
    (216, 160, 119),
    (156, 183, 155),
    (214, 176, 114),
    (157, 176, 201),
    (194, 160, 186),
    (141, 186, 180),
    (194, 169, 138),
    (166, 174, 182),
];

fn app_tint(key: &str, dark: bool) -> slint::Color {
    let hash = key
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32)) as usize;
    let (r, g, b) = if dark {
        PALETTE_DARK[hash % PALETTE_DARK.len()]
    } else {
        PALETTE_LIGHT[hash % PALETTE_LIGHT.len()]
    };
    slint::Color::from_rgb_u8(r, g, b)
}

fn initial(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "•".into())
}

fn peak_label(hour: u32) -> String {
    let next = (hour + 1) % 24;
    let a = if hour == 0 {
        12
    } else if hour < 12 {
        hour
    } else {
        hour - 12
    };
    let b = if next == 0 {
        12
    } else if next < 12 {
        next
    } else {
        next - 12
    };
    let suffix = if hour < 12 && next != 0 { "AM" } else { "PM" };
    format!("Peak {a}–{b} {suffix}")
}

pub struct AppController {
    pub ui: AppWindow,
    store: Option<Store>,
    tracker: Option<TrackerHandle>,
    settings: Settings,
    selected_offset: i32,
    view: i32,
    query: String,
    icons: IconCache,
    dark: bool,
    load_error: String,
    settings_error: String,
    last_live: LiveState,
    last_db_version: u64,
    last_elapsed_bucket: i64,
    last_day: Day,
    force_refresh: bool,
    last_refresh: Instant,
    settings_open: bool,
    setup_open: bool,
    setup_ready: bool,
    setup_busy: bool,
    setup_job: Arc<Mutex<Option<Result<crate::platform::Capabilities, String>>>>,
    setup_last_check: Instant,
    theme_signal: Arc<AtomicBool>,
    theme_watch_live: bool,
    last_theme_poll: Instant,
    restore_maximized: bool,
    command: Option<Arc<AtomicU8>>,
    /// Seconds left on the current timed pause, or 0 when there is none.
    pause_remaining: i64,
    last_pause_status: String,
}

impl AppController {
    pub fn new(
        ui: AppWindow,
        store: Option<Store>,
        tracker: Option<TrackerHandle>,
        settings: Settings,
        command: Option<Arc<AtomicU8>>,
    ) -> Rc<RefCell<Self>> {
        let dark = match settings.theme {
            ThemePref::Light => false,
            ThemePref::Dark => true,
            ThemePref::System => crate::platform::system_prefers_dark(),
        };
        let theme_signal = Arc::new(AtomicBool::new(false));
        let theme_watch_live = if settings.theme == ThemePref::System {
            crate::platform::watch_color_scheme(theme_signal.clone())
        } else {
            false
        };
        let load_error = if store.is_none() {
            "The local data file could not be opened.".to_string()
        } else {
            String::new()
        };
        let controller = Rc::new(RefCell::new(Self {
            ui,
            store,
            tracker,
            settings,
            selected_offset: 0,
            view: 0,
            query: String::new(),
            icons: IconCache::new(),
            dark,
            load_error,
            settings_error: String::new(),
            last_live: LiveState::default(),
            last_db_version: 0,
            last_elapsed_bucket: -1,
            last_day: Day::today(),
            force_refresh: true,
            last_refresh: Instant::now() - Duration::from_secs(10),
            settings_open: false,
            setup_open: false,
            setup_ready: false,
            setup_busy: false,
            setup_job: Arc::new(Mutex::new(None)),
            setup_last_check: Instant::now(),
            theme_signal,
            theme_watch_live,
            last_theme_poll: Instant::now(),
            restore_maximized: false,
            command,
            pause_remaining: 0,
            last_pause_status: String::new(),
        }));

        if let Some(icon) = search_icon() {
            controller.borrow().ui.set_search_icon(icon);
        }
        Self::wire(&controller);
        controller.borrow_mut().refresh();
        controller
    }

    pub fn refresh(&mut self) {
        self.ui.set_load_error(self.load_error.clone().into());
        if let Some(store) = &self.store {
            self.ui.set_recovered(store.recovered_corrupt_file);
        }
        self.ui.set_app_version(env!("CARGO_PKG_VERSION").into());
        self.ui.set_is_wayland(crate::platform::is_wayland());
        self.ui.set_selected_offset(self.selected_offset);
        self.ui.set_view(self.view);
        self.ui.set_maximized(self.ui.window().is_maximized());
        self.ui.global::<Theme>().set_dark(self.dark);
        self.sync_pause_from_tracker();
        self.publish_pause_state();
        self.ui.set_idle_seconds(self.settings.idle_seconds as i32);
        self.ui
            .set_retention_days(self.settings.retention_days as i32);
        self.ui.set_theme_pref(self.settings.theme.index());
        self.ui.set_autostart(self.settings.autostart);
        self.ui
            .set_settings_error(self.settings_error.clone().into());
        self.ui.set_setup_ready(self.setup_ready);
        self.ui.set_setup_busy(self.setup_busy);
        self.ui.set_tracking_title(self.tracking_title().into());
        self.ui
            .set_tracking_detail(self.last_live.detail.clone().into());
        self.ui.set_tracking_code(self.last_live.status.code());
        self.update_banner();

        if self.load_error.is_empty() && self.store.is_none() {
            self.ui
                .set_load_error("The local data file could not be opened.".into());
        }

        let today = Day::today();
        self.last_day = today;
        let day = today.add_days(self.selected_offset);
        let live = self
            .tracker
            .as_ref()
            .map(|t| t.snapshot())
            .unwrap_or_default();
        self.last_live = live.clone();
        self.last_db_version = live.db_version;
        self.last_elapsed_bucket = live.current.as_ref().map(|c| c.elapsed / 30).unwrap_or(-1);
        self.ui.set_tracking_title(self.tracking_title().into());
        self.ui.set_tracking_detail(live.detail.clone().into());
        self.ui.set_tracking_code(live.status.code());
        self.update_banner();

        let Some(store) = &self.store else {
            self.ui.set_total_text("0m".into());
            self.ui
                .set_compare_text("Daymark couldn’t open its local data file.".into());
            self.ui
                .set_week(ModelRc::new(VecModel::from(Vec::<DayBar>::new())));
            self.ui
                .set_apps(ModelRc::new(VecModel::from(Vec::<AppRow>::new())));
            self.ui
                .set_hours(ModelRc::new(VecModel::from(Vec::<HourBar>::new())));
            self.ui
                .set_history(ModelRc::new(VecModel::from(Vec::<HistoryRow>::new())));
            self.last_refresh = Instant::now();
            self.force_refresh = false;
            return;
        };

        // A query window large enough for the weekly chart, the previous day
        // and a seven-day average, without touching the whole database.
        let query_start = day.add_days(-8).start_ts();
        let query_end = day.end_ts();
        let mut rows = store
            .sessions_between(query_start, query_end)
            .unwrap_or_default();
        let mut live_info: Option<AppInfo> = None;
        if let Some(current) = &live.current {
            let end = current.started.saturating_add(current.elapsed.max(0));
            if end > current.started && end > query_start && current.started < query_end {
                rows.push(SessionRow {
                    app_id: current.app_id,
                    started: current.started,
                    ended: end,
                });
            }
            live_info = Some(AppInfo {
                id: current.app_id,
                key: current.key.clone(),
                name: current.name.clone(),
                icon: current.icon.clone(),
                desktop_id: None,
            });
        }
        let mut app_infos = {
            let ids: Vec<i64> = {
                let mut seen = std::collections::HashSet::new();
                rows.iter()
                    .filter(|r| seen.insert(r.app_id))
                    .map(|r| r.app_id)
                    .collect()
            };
            store.app_infos(&ids)
        };
        if let Some(info) = live_info {
            app_infos.entry(info.id).or_insert(info);
        }

        let day_agg = stats::aggregate_day(&rows, day);
        self.ui
            .set_total_text(date::format_duration(day_agg.total).into());
        self.ui.set_has_any_data(store.has_any_sessions());
        self.ui.set_date_kicker(
            if self.selected_offset == 0 {
                format!("TODAY  ·  {}", day.pretty().to_uppercase())
            } else {
                day.pretty().to_uppercase()
            }
            .into(),
        );
        self.ui.set_compare_text(
            self.comparison_text(&rows, day, day_agg.total, store)
                .into(),
        );

        let week_totals = stats::totals_ending_at(&rows, day, 7);
        let max_week = week_totals
            .iter()
            .map(|(_, v)| *v)
            .max()
            .unwrap_or(0)
            .max(1);
        let week_rows: Vec<DayBar> = week_totals
            .iter()
            .map(|(d, seconds)| {
                let offset = today_offset(today, *d);
                DayBar {
                    label: d.short().into(),
                    date_label: d.day_label().into(),
                    value: date::format_compact(*seconds).into(),
                    ratio: (*seconds as f32 / max_week as f32).clamp(0.0, 1.0),
                    selected: *d == day,
                    is_today: *d == today,
                    offset,
                }
            })
            .collect();
        let week_sum: i64 = week_totals.iter().map(|(_, v)| *v).sum();
        self.ui.set_week_label(
            if self.selected_offset == 0 {
                "Last 7 days".into()
            } else {
                format!("7 days to {}", day.short())
            }
            .into(),
        );
        self.ui.set_week_total_text(
            if week_sum > 0 {
                date::format_duration(week_sum)
            } else {
                String::new()
            }
            .into(),
        );
        self.ui.set_week(ModelRc::new(VecModel::from(week_rows)));

        // Application rows, including the current live interval.
        let total_apps = day_agg.apps.len();
        let query_lower = self.query.to_lowercase();
        let top = day_agg.apps.first().map(|(_, v)| *v).unwrap_or(1).max(1);
        let mut app_rows = Vec::new();
        for (app_id, seconds) in &day_agg.apps {
            let Some(info) = app_infos.get(app_id) else {
                continue;
            };
            if !query_lower.is_empty() && !info.name.to_lowercase().contains(&query_lower) {
                continue;
            }
            let tint = app_tint(&info.key, self.dark);
            let (icon, has_icon) = match info.icon.as_deref().and_then(|i| self.icons.get(i)) {
                Some(image) => (image, true),
                None => (Image::default(), false),
            };
            app_rows.push(AppRow {
                name: info.name.clone().into(),
                duration: date::format_duration(*seconds).into(),
                ratio: (*seconds as f32 / top as f32).clamp(0.0, 1.0),
                initial: initial(&info.name).into(),
                has_icon,
                icon,
                tint,
            });
        }
        let shown = app_rows.len();
        self.ui.set_apps_caption(
            if total_apps == 0 {
                String::new()
            } else if !self.query.is_empty() {
                format!("{shown} of {total_apps} apps")
            } else if total_apps == 1 {
                "1 app".into()
            } else {
                format!("{total_apps} apps")
            }
            .into(),
        );
        self.ui.set_apps(ModelRc::new(VecModel::from(app_rows)));

        let max_hour = day_agg.hourly.iter().copied().max().unwrap_or(0).max(1);
        let hours: Vec<HourBar> = day_agg
            .hourly
            .iter()
            .map(|v| HourBar {
                ratio: (*v as f32 / max_hour as f32).clamp(0.0, 1.0),
            })
            .collect();
        self.ui.set_hours(ModelRc::new(VecModel::from(hours)));
        self.ui.set_peak_text(
            day_agg
                .peak_hour()
                .map(|(h, _)| peak_label(h as u32))
                .unwrap_or_else(|| "No activity yet".into())
                .into(),
        );

        // History: a calm, scannable two-week list.
        let history_start = today.add_days(-(HISTORY_DAYS - 1)).start_ts();
        let mut history_rows = store
            .sessions_between(history_start, today.end_ts())
            .unwrap_or_default();
        if let Some(current) = &live.current {
            let end = current.started.saturating_add(current.elapsed.max(0));
            if end > history_start {
                history_rows.push(SessionRow {
                    app_id: current.app_id,
                    started: current.started,
                    ended: end,
                });
            }
        }
        let history_totals = stats::totals_ending_at(&history_rows, today, HISTORY_DAYS as i64);
        let max_history = history_totals
            .iter()
            .map(|(_, v)| *v)
            .max()
            .unwrap_or(0)
            .max(1);
        let active_days = history_totals.iter().filter(|(_, v)| *v > 0).count();
        let history_sum: i64 = history_totals.iter().map(|(_, v)| *v).sum();
        let history_model: Vec<HistoryRow> = history_totals
            .iter()
            .rev()
            .map(|(d, seconds)| HistoryRow {
                label: if *d == today {
                    "Today".to_string()
                } else if *d == today.add_days(-1) {
                    "Yesterday".to_string()
                } else {
                    d.weekday_name()
                }
                .into(),
                date: d.day_month().into(),
                duration: date::format_duration(*seconds).into(),
                ratio: (*seconds as f32 / max_history as f32).clamp(0.0, 1.0),
                is_today: *d == today,
                offset: today_offset(today, *d),
            })
            .collect();
        self.ui
            .set_history_total(date::format_duration(history_sum).into());
        self.ui.set_has_history(history_sum > 0);
        self.ui.set_history_summary(
            if active_days > 0 {
                let avg = history_sum / active_days.max(1) as i64;
                format!(
                    "Across {} {} · {} a day",
                    active_days,
                    if active_days == 1 { "day" } else { "days" },
                    date::format_duration(avg)
                )
            } else {
                String::new()
            }
            .into(),
        );
        self.ui
            .set_history(ModelRc::new(VecModel::from(history_model)));

        self.last_refresh = Instant::now();
        self.force_refresh = false;
    }

    /// The tracker applies a timed pause's expiry on its own thread,
    /// possibly while the window is hidden. Adopt its pause state so the
    /// interface reflects a resume that happened in the background.
    fn sync_pause_from_tracker(&mut self) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        let live_settings = tracker.settings();
        if live_settings.paused != self.settings.paused
            || live_settings.paused_until != self.settings.paused_until
        {
            self.settings.paused = live_settings.paused;
            self.settings.paused_until = live_settings.paused_until;
        }
    }

    /// The pause state as the interface should show it: whether tracking
    /// appears paused, and the seconds left on a timed pause. A deadline
    /// that has already passed is shown as resumed; the tracker confirms
    /// it within one tick.
    fn pause_state(&self) -> (bool, i64) {
        let settings = &self.settings;
        if !settings.paused {
            return (false, 0);
        }
        let now = crate::date::unix_now();
        if settings.paused_until > now {
            (true, settings.paused_until - now)
        } else if settings.paused_until == 0 {
            (true, 0)
        } else {
            (false, 0)
        }
    }

    /// Push the pause state, including the remaining time of a timed
    /// pause, to the interface.
    fn publish_pause_state(&mut self) {
        let (paused, remaining) = self.pause_state();
        self.pause_remaining = remaining;
        self.ui.set_paused(paused);
        self.ui.set_pause_remaining(remaining as i32);
        let status = pause_status_line(paused, remaining);
        self.last_pause_status = status.clone();
        self.ui.set_pause_status(status.into());
    }

    fn comparison_text(&self, rows: &[SessionRow], day: Day, total: i64, store: &Store) -> String {
        if total <= 0 {
            if !store.has_any_sessions() && self.selected_offset == 0 {
                return "Nothing recorded yet — usage will appear as you work.".into();
            }
            return if self.selected_offset == 0 {
                "No activity recorded today.".into()
            } else {
                "No activity recorded on this day.".into()
            };
        }
        let yesterday = day.add_days(-1);
        let previous = stats::total_for_day(rows, yesterday);
        let relation = if self.selected_offset == 0 {
            "yesterday"
        } else {
            "the day before"
        };
        if previous > 0 {
            let diff = total - previous;
            if diff.abs() < 300 {
                return format!("About the same as {relation}.");
            }
            return format!(
                "{} {} than {relation}.",
                date::format_delta(diff),
                if diff > 0 { "more" } else { "less" }
            );
        }
        // Only use an average when there is enough real history to mean it.
        let prior = stats::totals_ending_at(rows, yesterday, 7);
        if let Some(avg) = stats::active_day_average(&prior, 3) {
            let diff = total - avg;
            if diff.abs() < 300 {
                return "About your daily average.".into();
            }
            return format!(
                "{} {} than your daily average.",
                date::format_delta(diff),
                if diff > 0 { "more" } else { "less" }
            );
        }
        "Your history is just beginning.".into()
    }

    fn tracking_title(&self) -> String {
        match self.last_live.status {
            TrackingStatus::Active => "Tracking is on".into(),
            TrackingStatus::Away => "Taking a break".into(),
            TrackingStatus::Paused => "Tracking is paused".into(),
            TrackingStatus::NeedsSetup => "Tracking needs setup".into(),
            TrackingStatus::Waiting => "Waiting for GNOME".into(),
            TrackingStatus::Stale => "Tracking stopped".into(),
            TrackingStatus::Error => "Something needs attention".into(),
        }
    }

    fn update_banner(&mut self) {
        // The pause banner says when a timed pause ends on its own.
        let paused_body = if self.pause_remaining > 0 {
            format!(
                "No new time is being counted. Tracking resumes on its own in {}.",
                date::format_duration(self.pause_remaining)
            )
        } else {
            "No new time is being counted until you resume.".to_string()
        };
        let (show, title, body, action, kind) = match self.last_live.status {
            TrackingStatus::NeedsSetup => (
                true,
                "Turn on app tracking",
                "Daymark needs its small GNOME companion to see which app is active. It never reads window titles or keystrokes.",
                "Set up tracking",
                0,
            ),
            TrackingStatus::Waiting => (
                true,
                "One last step",
                "The companion is installed. Sign out and back in once so GNOME can load it.",
                "Details",
                3,
            ),
            TrackingStatus::Stale => (
                true,
                "Tracking paused itself",
                "The GNOME companion stopped reporting. You can install it again from here.",
                "Set up again",
                0,
            ),
            TrackingStatus::Paused => (
                true,
                "Tracking is paused",
                paused_body.as_str(),
                "Resume",
                1,
            ),
            TrackingStatus::Error => (
                true,
                "The tracker needs attention",
                self.last_live.detail.as_str(),
                "Try again",
                2,
            ),
            TrackingStatus::Active | TrackingStatus::Away => (false, "", "", "", 0),
        };
        let show = show && !self.setup_open && !self.settings_open;
        self.ui.set_show_banner(show);
        self.ui.set_banner_title(title.into());
        self.ui.set_banner_body(body.into());
        self.ui.set_banner_action(action.into());
        self.ui.set_banner_kind(kind);
    }

    fn wire(controller: &Rc<RefCell<Self>>) {
        // Registered explicitly: Slint's callback setter names are generated
        // from hyphens, and explicit code keeps the borrows obvious.
        {
            let c = controller.clone();
            controller.borrow().ui.on_nav_day(move |delta| {
                let mut ctl = c.borrow_mut();
                ctl.selected_offset = (ctl.selected_offset + delta).clamp(-3650, 0);
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_go_today(move || {
                let mut ctl = c.borrow_mut();
                ctl.selected_offset = 0;
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_select_day(move |offset| {
                let mut ctl = c.borrow_mut();
                ctl.selected_offset = offset.clamp(-3650, 0);
                ctl.view = 0;
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_set_view(move |view| {
                let mut ctl = c.borrow_mut();
                ctl.view = if view == 1 { 1 } else { 0 };
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_open_settings(move || {
                let mut ctl = c.borrow_mut();
                ctl.settings_open = true;
                ctl.setup_open = false;
                ctl.ui.set_settings_open(true);
                ctl.ui.set_setup_open(false);
                ctl.ui.set_confirm_delete(false);
                ctl.settings_error.clear();
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_close_settings(move || {
                let mut ctl = c.borrow_mut();
                ctl.settings_open = false;
                ctl.ui.set_settings_open(false);
                ctl.ui.set_confirm_delete(false);
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_open_setup(move || {
                let mut ctl = c.borrow_mut();
                ctl.setup_open = true;
                ctl.ui.set_setup_open(true);
                ctl.setup_last_check = Instant::now() - Duration::from_secs(5);
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_close_setup(move || {
                let mut ctl = c.borrow_mut();
                ctl.setup_open = false;
                ctl.ui.set_setup_open(false);
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_setup_tracking(move || {
                let mut ctl = c.borrow_mut();
                ctl.begin_setup();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_resume_tracking(move || {
                let mut ctl = c.borrow_mut();
                ctl.settings.paused = false;
                ctl.settings.paused_until = 0;
                ctl.save_settings();
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_retry(move || {
                let mut ctl = c.borrow_mut();
                ctl.retry_backend();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_set_idle(move |seconds| {
                let mut ctl = c.borrow_mut();
                ctl.settings.idle_seconds = (seconds as i64).clamp(60, 3600);
                ctl.save_settings();
                settings::write_extension_config(&db::runtime_dir(), ctl.settings.idle_seconds);
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_set_retention(move |days| {
                let mut ctl = c.borrow_mut();
                ctl.settings.retention_days = if days <= 0 {
                    0
                } else {
                    (days as i64).clamp(30, 3650)
                };
                ctl.save_settings();
                if let Some(store) = &ctl.store {
                    let cutoff = Day::today()
                        .add_days(-(ctl.settings.retention_days as i32))
                        .start_ts();
                    if ctl.settings.retention_days > 0 {
                        if let Err(e) = store.prune_before(cutoff) {
                            logging::error("retention", &e);
                        }
                    }
                }
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_set_theme(move |pref| {
                let mut ctl = c.borrow_mut();
                ctl.settings.theme = match pref {
                    1 => ThemePref::Light,
                    2 => ThemePref::Dark,
                    _ => ThemePref::System,
                };
                ctl.dark = match ctl.settings.theme {
                    ThemePref::Light => false,
                    ThemePref::Dark => true,
                    ThemePref::System => crate::platform::system_prefers_dark(),
                };
                ctl.ensure_theme_watcher();
                ctl.save_settings();
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_set_autostart(move |enabled| {
                let mut ctl = c.borrow_mut();
                ctl.settings.autostart = enabled;
                match crate::platform::apply_autostart(enabled) {
                    Ok(()) => {
                        ctl.settings_error.clear();
                        ctl.save_settings();
                    }
                    Err(e) => {
                        logging::error("autostart", &e);
                        ctl.settings.autostart = crate::platform::autostart_installed();
                        ctl.settings_error = e;
                    }
                }
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            // `seconds` is the pause length; 0 means "until resumed".
            controller.borrow().ui.on_pause_for(move |seconds| {
                let mut ctl = c.borrow_mut();
                ctl.settings.paused = true;
                ctl.settings.paused_until = if seconds > 0 {
                    crate::date::unix_now() + seconds as i64
                } else {
                    0
                };
                ctl.save_settings();
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_search_changed(move |text| {
                let mut ctl = c.borrow_mut();
                ctl.query = text.to_string();
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_open_data(move || {
                let mut ctl = c.borrow_mut();
                if let Err(e) = crate::platform::open_data_folder() {
                    logging::error("open_data_folder", &e);
                    ctl.settings_error = e;
                    ctl.force_refresh = true;
                    ctl.refresh();
                }
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_delete_all(move || {
                let mut ctl = c.borrow_mut();
                if let Some(tracker) = &ctl.tracker {
                    tracker.request_reset();
                } else if let Some(store) = &ctl.store {
                    if let Err(e) = store.delete_all() {
                        logging::error("delete_all", &e);
                    }
                }
                ctl.query.clear();
                ctl.ui.set_search_text("".into());
                ctl.settings_error.clear();
                ctl.force_refresh = true;
                ctl.refresh();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_hide_window(move || {
                let mut ctl = c.borrow_mut();
                ctl.restore_maximized = ctl.ui.window().is_maximized();
                let _ = ctl.ui.window().hide_without_quitting();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_toggle_maximize(move || {
                let ctl = c.borrow();
                let window = ctl.ui.window();
                let next = !window.is_maximized();
                window.set_maximized(next);
                ctl.ui.set_maximized(next);
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_minimize_window(move || {
                let ctl = c.borrow();
                ctl.ui.window().set_minimized(true);
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_start_window_drag(move || {
                let ctl = c.borrow();
                ctl.ui.window().start_window_move();
            });
        }
        {
            let c = controller.clone();
            controller.borrow().ui.on_quit(move || {
                c.borrow_mut().quit();
            });
        }
        // Alt+F4 and compositor close requests behave like the ✕ control:
        // hide to the background and keep tracking, never quit silently.
        {
            let c = controller.clone();
            controller.borrow().ui.window().on_close_requested(move || {
                if let Ok(mut ctl) = c.try_borrow_mut() {
                    ctl.restore_maximized = ctl.ui.window().is_maximized();
                    let _ = ctl.ui.window().hide_without_quitting();
                }
                slint::CloseRequestResponse::KeepWindowShown
            });
        }
    }

    fn save_settings(&mut self) {
        if let Some(store) = &self.store {
            if let Err(e) = self.settings.save(store) {
                logging::error("save_settings", &e);
                self.settings_error = format!("Could not save settings: {e}");
            } else {
                self.settings_error.clear();
            }
        }
        if let Some(tracker) = &self.tracker {
            tracker.set_settings(self.settings.clone());
        }
    }

    fn retry_backend(&mut self) {
        match Store::open_default() {
            Ok(store) => {
                self.load_error.clear();
                self.settings = Settings::load(&store);
                self.store = Some(store);
                if self.tracker.is_none() {
                    match Store::open_default() {
                        Ok(write_store) => {
                            self.tracker = Some(tracker::spawn(write_store, self.settings.clone()));
                        }
                        Err(e) => {
                            logging::error("tracking_database", &e);
                            self.settings_error = e;
                        }
                    }
                }
                if let Err(e) = crate::platform::apply_autostart(self.settings.autostart) {
                    logging::error("autostart", &e);
                    self.settings_error = e;
                }
                self.force_refresh = true;
                self.refresh();
            }
            Err(e) => {
                logging::error("database", &e);
                self.load_error = e;
                self.refresh();
            }
        }
    }

    fn quit(&mut self) {
        if let Some(tracker) = &self.tracker {
            tracker.shutdown();
        }
        let _ = slint::quit_event_loop();
    }

    /// Called by the UI timer. Only refreshes when something visible changed.
    pub fn tick(&mut self) {
        if let Some(command) = &self.command {
            match command.swap(COMMAND_NONE, Ordering::SeqCst) {
                COMMAND_SHOW => {
                    self.force_refresh = true;
                    self.last_theme_poll = Instant::now() - Duration::from_secs(30);
                    self.theme_signal.store(true, Ordering::Relaxed);
                    let window = self.ui.window();
                    window.set_minimized(false);
                    let _ = self.ui.show();
                    if self.restore_maximized {
                        window.set_maximized(true);
                    }
                }
                COMMAND_QUIT => {
                    self.quit();
                    return;
                }
                _ => {}
            }
        }
        if let Some(result) = self.setup_job.lock().ok().and_then(|mut job| job.take()) {
            self.finish_setup(result);
        }
        self.apply_system_theme();
        if self.setup_open
            && !self.setup_ready
            && !self.setup_busy
            && self.setup_last_check.elapsed() > Duration::from_secs(2)
        {
            self.setup_last_check = Instant::now();
            if crate::platform::state_age_secs().is_some_and(|age| age < 45) {
                self.setup_ready = true;
                self.ui.set_setup_ready(true);
                self.force_refresh = true;
            }
        }
        let is_maximized = self.ui.window().is_maximized();
        if self.ui.get_maximized() != is_maximized {
            self.ui.set_maximized(is_maximized);
        }
        if !self.ui.window().is_visible() {
            return;
        }
        // While a pause runs, refresh when its status line changes — the
        // remaining time moves a minute at a time.
        if self.settings.paused {
            let (paused_now, remaining_now) = self.pause_state();
            let status = pause_status_line(paused_now, remaining_now);
            if status != self.last_pause_status {
                self.last_pause_status = status;
                self.force_refresh = true;
            }
        }
        if self.force_refresh {
            self.refresh();
            return;
        }
        let live = self
            .tracker
            .as_ref()
            .map(|t| t.snapshot())
            .unwrap_or_default();
        let bucket = live.current.as_ref().map(|c| c.elapsed / 30).unwrap_or(-1);
        let changed = live.db_version != self.last_db_version
            || live.status != self.last_live.status
            || live.detail != self.last_live.detail
            || bucket != self.last_elapsed_bucket
            || Day::today() != self.last_day;
        if changed && self.last_refresh.elapsed() > Duration::from_millis(350) {
            self.last_live = live;
            self.refresh();
        }
    }

    /// Follow GNOME's colour scheme while the preference is "System". The
    /// companion `gsettings monitor` thread only raises a flag, so the actual
    /// read and every Slint mutation stay on the UI thread.
    fn apply_system_theme(&mut self) {
        if self.settings.theme != ThemePref::System {
            return;
        }
        let notified = self.theme_signal.swap(false, Ordering::Relaxed);
        let sample =
            !self.theme_watch_live && self.last_theme_poll.elapsed() > Duration::from_secs(10);
        if !notified && !sample {
            return;
        }
        self.last_theme_poll = Instant::now();
        let dark = crate::platform::system_prefers_dark();
        if dark != self.dark {
            self.dark = dark;
            self.ui.global::<Theme>().set_dark(dark);
            self.force_refresh = true;
        }
    }

    /// Start the colour-scheme monitor when the user switches back to System.
    fn ensure_theme_watcher(&mut self) {
        if self.settings.theme != ThemePref::System || self.theme_watch_live {
            return;
        }
        self.theme_watch_live = crate::platform::watch_color_scheme(self.theme_signal.clone());
        self.last_theme_poll = Instant::now() - Duration::from_secs(30);
    }

    fn begin_setup(&mut self) {
        if self.setup_busy {
            return;
        }
        self.settings_error.clear();
        self.setup_busy = true;
        self.ui.set_setup_busy(true);
        self.force_refresh = true;
        self.refresh();
        let job = self.setup_job.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("daymark-setup".into())
            .spawn(move || {
                let result = crate::platform::install_extension();
                if let Ok(mut slot) = job.lock() {
                    *slot = Some(result);
                }
            })
        {
            logging::error("setup_thread", &error.to_string());
            self.setup_busy = false;
            self.ui.set_setup_busy(false);
            self.settings_error = "Could not start the setup helper.".into();
            self.force_refresh = true;
            self.refresh();
        }
    }

    fn finish_setup(&mut self, result: Result<crate::platform::Capabilities, String>) {
        self.setup_busy = false;
        self.ui.set_setup_busy(false);
        match result {
            Ok(caps) => {
                self.setup_ready = caps.state_fresh;
                self.setup_open = true;
                self.ui.set_setup_open(true);
                self.ui.set_setup_ready(caps.state_fresh);
                self.settings.autostart = true;
                if let Some(store) = &self.store {
                    let _ = self.settings.save(store);
                }
                let _ = crate::platform::apply_autostart(true);
            }
            Err(e) => {
                logging::error("extension_install", &e);
                self.settings_error = e;
            }
        }
        self.force_refresh = true;
        self.refresh();
    }

    pub fn set_load_error(&mut self, message: String) {
        self.load_error = message;
        self.force_refresh = true;
        self.refresh();
    }

    pub fn set_external_error(&mut self, message: String) {
        self.settings_error = message;
        self.force_refresh = true;
        self.refresh();
    }

    pub fn tracker_handle(&self) -> Option<TrackerHandle> {
        self.tracker.clone()
    }

    pub fn set_view_for_render(&mut self, view: i32) {
        self.view = view;
        self.ui.set_view(view);
        self.force_refresh = true;
        self.refresh();
    }

    pub fn open_settings_for_render(&mut self) {
        self.settings_open = true;
        self.ui.set_settings_open(true);
        self.force_refresh = true;
        self.refresh();
    }

    pub fn open_setup_for_render(&mut self, ready: bool) {
        self.setup_open = true;
        self.setup_ready = ready;
        self.ui.set_setup_open(true);
        self.ui.set_setup_ready(ready);
        self.force_refresh = true;
        self.refresh();
    }

    pub fn set_selected_for_render(&mut self, offset: i32) {
        self.selected_offset = offset;
        self.force_refresh = true;
        self.refresh();
    }
}

fn today_offset(today: Day, day: Day) -> i32 {
    // Day boundaries are local, so round the difference to the nearest day.
    let diff = day.start_ts() - today.start_ts();
    (diff as f64 / 86_400.0).round() as i32
}

/// One-line description of the pause state for the settings panel and the
/// background-resume path: empty while tracking runs.
fn pause_status_line(paused: bool, remaining: i64) -> String {
    if !paused {
        return String::new();
    }
    if remaining > 0 {
        format!("Paused — {} remaining", date::format_duration(remaining))
    } else {
        "Paused until you resume".to_string()
    }
}
