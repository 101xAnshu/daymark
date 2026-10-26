//! The tracking engine. It owns the write connection, reads the small state
//! file published by the GNOME extension, and converts focus changes into
//! non-overlapping intervals.

use std::collections::HashMap;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use inotify::{Inotify, WatchMask};

use crate::date::{self, Day};
use crate::db::Store;
use crate::desktop::{DesktopIndex, ResolvedApp};
use crate::platform;
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingStatus {
    Active,
    Away,
    Paused,
    NeedsSetup,
    Waiting,
    Stale,
    Error,
}

impl TrackingStatus {
    pub fn code(self) -> i32 {
        match self {
            TrackingStatus::Active => 0,
            TrackingStatus::Away => 1,
            TrackingStatus::Paused => 2,
            TrackingStatus::NeedsSetup => 3,
            TrackingStatus::Waiting => 4,
            TrackingStatus::Stale => 5,
            TrackingStatus::Error => 6,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LiveCurrent {
    pub app_id: i64,
    pub key: String,
    pub name: String,
    pub icon: Option<String>,
    pub started: i64,
    pub elapsed: i64,
}

#[derive(Clone, Debug)]
pub struct LiveState {
    pub current: Option<LiveCurrent>,
    pub status: TrackingStatus,
    pub detail: String,
    pub db_version: u64,
    /// Set when the extension has produced at least one valid state file.
    pub ever_saw_state: bool,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            current: None,
            status: TrackingStatus::NeedsSetup,
            detail: "GNOME integration not detected".into(),
            db_version: 0,
            ever_saw_state: false,
        }
    }
}

#[derive(Clone)]
pub struct TrackerHandle {
    pub live: Arc<Mutex<LiveState>>,
    pub settings: Arc<Mutex<Settings>>,
    pub quit: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    join: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl TrackerHandle {
    /// A handle without a working thread, for the screenshot renderer.
    pub fn demo(settings: Settings, status: TrackingStatus, detail: &str) -> Self {
        let live = LiveState {
            current: None,
            status,
            detail: detail.to_string(),
            db_version: 1,
            ever_saw_state: status == TrackingStatus::Active,
        };
        Self {
            live: Arc::new(Mutex::new(live)),
            settings: Arc::new(Mutex::new(settings)),
            quit: Arc::new(AtomicBool::new(false)),
            reset: Arc::new(AtomicBool::new(false)),
            join: Arc::new(Mutex::new(None)),
        }
    }

    pub fn snapshot(&self) -> LiveState {
        self.live.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn settings(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn set_settings(&self, settings: Settings) {
        let pause_changed = self
            .settings
            .lock()
            .map(|current| {
                current.paused != settings.paused || current.paused_until != settings.paused_until
            })
            .unwrap_or(false);
        if let Ok(mut current) = self.settings.lock() {
            *current = settings;
        }
        if pause_changed {
            // A pause or resume takes effect at once instead of at the next
            // two-second tick, so no further time is counted after a pause.
            self.wake();
        }
    }
    pub fn stop(&self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Ok(slot) = self.join.lock() {
            if let Some(handle) = slot.as_ref() {
                handle.thread().unpark();
            }
        }
    }
    /// Ask the tracker to discard all recorded intervals. It happens on the
    /// tracking thread so an open interval can never be re-created afterwards.
    pub fn request_reset(&self) {
        self.reset.store(true, Ordering::Relaxed);
        self.wake();
    }
    /// Stop and wait briefly for the final interval to be closed cleanly.
    pub fn shutdown(&self) {
        self.stop();
        if let Ok(mut slot) = self.join.lock() {
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
    }
    fn wake(&self) {
        if let Ok(slot) = self.join.lock() {
            if let Some(handle) = slot.as_ref() {
                handle.thread().unpark();
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ExtensionFocus {
    id: String,
    name: Option<String>,
    wm_class: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct ExtensionState {
    focus: Option<ExtensionFocus>,
    away: bool,
    away_since: i64,
    idle: bool,
    locked: bool,
}

struct StateReader {
    path: PathBuf,
    last_stamp: Option<SystemTime>,
    last_len: u64,
    cached: Option<ExtensionState>,
}

impl StateReader {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_stamp: None,
            last_len: 0,
            cached: None,
        }
    }

    fn poll(&mut self) -> Option<(ExtensionState, u64)> {
        let meta = std::fs::metadata(&self.path).ok()?;
        let stamp = meta.modified().ok()?;
        let len = meta.len();
        let age = SystemTime::now()
            .duration_since(stamp)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if self.last_stamp != Some(stamp) || self.last_len != len {
            self.cached = std::fs::read_to_string(&self.path)
                .ok()
                .and_then(|text| parse_state(&text));
            self.last_stamp = Some(stamp);
            self.last_len = len;
        }
        self.cached.clone().map(|state| (state, age))
    }
}

fn parse_state(text: &str) -> Option<ExtensionState> {
    let value = crate::json::parse(text)?;
    let version = value.get("v")?.as_i64()?;
    if version < 1 {
        return None;
    }
    let focus = value.get("focus").and_then(|f| {
        let id = f.get("id")?.as_str()?.to_string();
        if id.is_empty() || id == "wmclass:" {
            return None;
        }
        Some(ExtensionFocus {
            id,
            name: f.get("name").and_then(|n| n.as_str()).map(str::to_string),
            wm_class: f
                .get("wm_class")
                .and_then(|n| n.as_str())
                .map(str::to_string),
        })
    });
    let away_since = value
        .get("away_since")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    // v1 of the companion wrote milliseconds; v2 writes seconds. Normalise so
    // an upgraded extension and an old state file agree on the unit.
    let away_since = if away_since > 10_000_000_000 {
        away_since / 1000
    } else {
        away_since
    };
    Some(ExtensionState {
        focus,
        away: value.get("away").and_then(|v| v.as_bool()).unwrap_or(false),
        away_since,
        idle: value.get("idle").and_then(|v| v.as_bool()).unwrap_or(false),
        locked: value
            .get("locked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

/// A focus shorter than this is noise. It should not become its own row;
/// when it is contiguous with the preceding session it is merged into that
/// session's row, and otherwise it is discarded.
const NOISE_FLOOR_SECS: i64 = 5;

/// Small slack for comparing the stored end of the previous session with the
/// start of the next one: integer-second accounting can differ by one.
const CONTIGUOUS_SLACK: i64 = 1;

/// The companion writes on every focus, idle and lock change and sends a
/// heartbeat every 15 seconds. An inotify wake handles those at once; this is
/// only the safety net for a missed event.
const WATCH_TIMEOUT: Duration = Duration::from_secs(30);

/// The pre-watch pace, kept for the fallback when no watch can be installed.
const FALLBACK_POLL: Duration = Duration::from_secs(2);

/// Close-write covers an in-place write; moved-to, create, delete and attrib
/// cover an atomic replacement, a deletion and a permission change.
const WATCH_MASK: WatchMask = WatchMask::CLOSE_WRITE
    .union(WatchMask::MOVED_TO)
    .union(WatchMask::CREATE)
    .union(WatchMask::DELETE)
    .union(WatchMask::ATTRIB);

/// One tick may credit at most one full watch interval. A larger monotonic
/// jump is still treated as a gap, so a missed suspend signal cannot turn
/// sleep into usage.
const MAX_CREDIT_SECS: i64 = WATCH_TIMEOUT.as_secs() as i64;
const TICK_GAP_SECS: i64 = MAX_CREDIT_SECS + 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitOutcome {
    Event,
    Timeout,
}

/// The pure part of the wait decision: a positive poll result is an event,
/// zero is a timeout, and an error is also retried after the fallback wait.
fn wait_outcome(ready: libc::c_int) -> WaitOutcome {
    if ready > 0 {
        WaitOutcome::Event
    } else {
        WaitOutcome::Timeout
    }
}

/// How long to block without a filesystem event: the watch safety net when
/// watching, the legacy poll when falling back.
fn wait_timeout(watching: bool) -> Duration {
    if watching {
        WATCH_TIMEOUT
    } else {
        FALLBACK_POLL
    }
}

/// A self-pipe so IPC wakeups interrupt `poll` the same way `unpark` used to
/// interrupt `park_timeout`.
struct WakePipe {
    read: OwnedFd,
    write: OwnedFd,
}

impl WakePipe {
    fn new() -> io::Result<Self> {
        let mut fds = [0; 2];
        let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        // The two descriptors are fresh and owned by this object.
        let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        Ok(Self { read, write })
    }

    fn wake(&self) {
        let byte = 1u8;
        let _ = unsafe {
            libc::write(
                self.write.as_raw_fd(),
                (&byte as *const u8).cast(),
                std::mem::size_of_val(&byte),
            )
        };
    }

    fn drain(&self) {
        let mut buffer = [0u8; 64];
        loop {
            let read = unsafe {
                libc::read(
                    self.read.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if read <= 0 {
                break;
            }
        }
    }
}

/// The runtime-directory watch plus the wake pipe. Without a watch it still
/// waits on the pipe with the fallback timeout, so IPC stays immediate.
struct RuntimeWatch {
    inotify: Option<Inotify>,
    wake: Arc<WakePipe>,
}

impl RuntimeWatch {
    fn new(directory: &Path, wake: Arc<WakePipe>) -> io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let inotify = Inotify::init()?;
        inotify.watches().add(directory, WATCH_MASK)?;
        Ok(Self {
            inotify: Some(inotify),
            wake,
        })
    }

    fn without_watch(wake: Arc<WakePipe>) -> Self {
        Self {
            inotify: None,
            wake,
        }
    }

    fn timeout(&self) -> Duration {
        wait_timeout(self.inotify.is_some())
    }

    fn wait(&mut self, timeout: Duration) -> WaitOutcome {
        let mut fds = [
            libc::pollfd {
                fd: self
                    .inotify
                    .as_ref()
                    .map(AsRawFd::as_raw_fd)
                    .unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.wake.read.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let millis = timeout.as_millis().min(libc::c_int::MAX as u128) as libc::c_int;
        let ready = loop {
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
            if ready < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break ready;
        };
        if fds[1].revents & libc::POLLIN != 0 {
            self.wake.drain();
        }
        if fds[0].revents & libc::POLLIN != 0 {
            self.drain_events();
        }
        wait_outcome(ready)
    }

    fn drain_events(&mut self) {
        let Some(inotify) = self.inotify.as_mut() else {
            return;
        };
        let mut buffer = [0u8; 4096];
        loop {
            match inotify.read_events(&mut buffer) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }
}

/// The most recently closed session that may still be extended: a following
/// short focus merges into it, and an immediate return to the same app
/// re-opens it instead of writing a duplicate adjacent row.
struct PrevSession {
    db_id: i64,
    app_id: i64,
    key: String,
    started: i64,
    ended: i64,
    /// False after a close caused by a gap, being away, a stale state file, a
    /// pause, or a midnight split: the time after such a close is never
    /// counted as belonging to this row.
    mergeable: bool,
}

struct OpenSession {
    db_id: i64,
    app_id: i64,
    key: String,
    name: String,
    icon: Option<String>,
    started: i64,
    accumulated: i64,
    last_heartbeat: i64,
}

struct Tracker {
    store: Store,
    reader: StateReader,
    index: DesktopIndex,
    index_loaded: Instant,
    resolved: HashMap<String, ResolvedApp>,
    current: Option<OpenSession>,
    previous: Option<PrevSession>,
    /// The noise floor in seconds; tests shrink it so old short sessions
    /// still count at the legacy pace.
    noise_floor: i64,
    last_tick_wall: i64,
    last_tick_mono: Instant,
    handle: TrackerHandle,
    last_prune_day: Option<Day>,
    capability: platform::Capabilities,
    ever_saw_state: bool,
    last_status: TrackingStatus,
    last_detail: String,
}

fn resolve_key(focus: &ExtensionFocus) -> String {
    format!("{}|{}", focus.id, focus.wm_class.as_deref().unwrap_or(""))
}

impl Tracker {
    fn new(store: Store, handle: TrackerHandle) -> Self {
        let capability = platform::capabilities();
        let ever_saw_state = capability.state_file_present;
        let (status, detail) = if capability.state_fresh {
            (TrackingStatus::Active, "Tracking is on".to_string())
        } else if !capability.extension_installed {
            (
                TrackingStatus::NeedsSetup,
                "GNOME integration is not installed".into(),
            )
        } else if !capability.extension_enabled {
            (
                TrackingStatus::NeedsSetup,
                "GNOME integration is not enabled".into(),
            )
        } else {
            (
                TrackingStatus::Waiting,
                "Sign out and back in once to activate tracking".into(),
            )
        };
        {
            if let Ok(mut live) = handle.live.lock() {
                live.status = status;
                live.detail = detail.clone();
                live.ever_saw_state = ever_saw_state;
            }
        }
        Self {
            store,
            reader: StateReader::new(platform::state_path()),
            index: DesktopIndex::load(),
            index_loaded: Instant::now(),
            resolved: HashMap::new(),
            current: None,
            previous: None,
            noise_floor: NOISE_FLOOR_SECS,
            last_tick_wall: date::unix_now(),
            last_tick_mono: Instant::now(),
            handle,
            last_prune_day: None,
            capability,
            ever_saw_state,
            last_status: status,
            last_detail: detail,
        }
    }

    fn set_status(&mut self, status: TrackingStatus, detail: impl Into<String>) {
        let detail = detail.into();
        if self.last_status == status && self.last_detail == detail {
            return;
        }
        self.last_status = status;
        self.last_detail = detail.clone();
        if let Ok(mut live) = self.handle.live.lock() {
            live.status = status;
            live.detail = detail;
            live.ever_saw_state = self.ever_saw_state;
        }
    }

    fn publish_current(&mut self) {
        let current = self.current.as_ref().map(|s| LiveCurrent {
            app_id: s.app_id,
            key: s.key.clone(),
            name: s.name.clone(),
            icon: s.icon.clone(),
            started: s.started,
            elapsed: s.accumulated,
        });
        if let Ok(mut live) = self.handle.live.lock() {
            live.current = current;
            live.db_version += 1;
            live.ever_saw_state = self.ever_saw_state;
        }
    }

    fn resolve(&mut self, focus: &ExtensionFocus) -> ResolvedApp {
        let key = resolve_key(focus);
        if let Some(app) = self.resolved.get(&key) {
            if app.matched || self.index_loaded.elapsed() < Duration::from_secs(60) {
                return app.clone();
            }
        }
        if self.index_loaded.elapsed() > Duration::from_secs(120) || self.index.is_empty() {
            self.index = DesktopIndex::load();
            self.index_loaded = Instant::now();
            self.resolved.clear();
        }
        let app = self
            .index
            .resolve(&focus.id, focus.wm_class.as_deref(), focus.name.as_deref());
        if self.resolved.len() > 512 {
            self.resolved.clear();
        }
        self.resolved.insert(key, app.clone());
        app
    }

    fn close_current(&mut self, end: i64, mergeable: bool) {
        let Some(session) = self.current.take() else {
            return;
        };
        let end = end.max(session.started);
        let duration = end - session.started;
        let day = Day::from_timestamp(session.started);
        // A short focus is noise: merge it into the preceding session when
        // that session is contiguous and was not separated from it by a gap,
        // being away, a stale state file, a pause, or a midnight split.
        let target = if duration < self.noise_floor {
            self.previous
                .as_ref()
                .filter(|prev| {
                    prev.mergeable
                        && Day::from_timestamp(prev.ended) == day
                        && session.started - prev.ended >= 0
                        && session.started - prev.ended <= CONTIGUOUS_SLACK
                })
                .map(|prev| (prev.db_id, end))
        } else {
            None
        };
        if let Some((target_id, end)) = target {
            if let Err(e) = self
                .store
                .merge_short_session(session.db_id, target_id, end)
            {
                // Close the open row no matter what so the open-session
                // invariant and crash recovery keep working.
                let _ = self.store.discard_session(session.db_id);
                self.set_status(TrackingStatus::Error, format!("Could not save usage: {e}"));
            } else if let Some(prev) = &mut self.previous {
                prev.ended = end;
            }
        } else if duration < self.noise_floor {
            // No contiguous mergeable row: a short focus becomes no row at
            // all. The real preceding session is untouched.
            let _ = self.store.discard_session(session.db_id);
        } else if let Err(e) = self.store.close_session(session.db_id, end) {
            self.set_status(TrackingStatus::Error, format!("Could not save usage: {e}"));
        } else {
            self.previous = Some(PrevSession {
                db_id: session.db_id,
                app_id: session.app_id,
                key: session.key,
                started: session.started,
                ended: end,
                mergeable,
            });
        }
        if let Ok(mut live) = self.handle.live.lock() {
            live.current = None;
            live.db_version += 1;
        }
    }

    /// When focus returns to the same application right after a short-lived
    /// interruption, continue the previous row instead of writing a second
    /// adjacent row for the same app. Never spans a midnight or a branch that
    /// marked the previous session unmergeable.
    fn try_resume_previous(&mut self, app: &ResolvedApp, now: i64) -> bool {
        let Some(prev) = self.previous.take() else {
            return false;
        };
        let contiguous = prev.ended <= now && now - prev.ended <= CONTIGUOUS_SLACK;
        if !(prev.mergeable
            && contiguous
            && prev.key == app.key
            && Day::from_timestamp(prev.ended) == Day::from_timestamp(now))
        {
            self.previous = Some(prev);
            return false;
        }
        if let Err(e) = self.store.reopen_session(prev.db_id) {
            self.previous = Some(prev);
            self.set_status(
                TrackingStatus::Error,
                format!("Could not update usage: {e}"),
            );
            return false;
        }
        self.current = Some(OpenSession {
            db_id: prev.db_id,
            app_id: prev.app_id,
            key: app.key.clone(),
            name: app.name.clone(),
            icon: app.icon.clone(),
            started: prev.started,
            accumulated: prev.ended - prev.started,
            last_heartbeat: prev.ended,
        });
        self.publish_current();
        true
    }

    fn start_session(&mut self, app: ResolvedApp, now: i64) {
        let app_id = match self.store.upsert_app(&app, now) {
            Ok(id) => id,
            Err(e) => {
                self.set_status(
                    TrackingStatus::Error,
                    format!("Could not save the app: {e}"),
                );
                return;
            }
        };
        match self.store.open_session(app_id, now) {
            Ok(db_id) => {
                self.current = Some(OpenSession {
                    db_id,
                    app_id,
                    key: app.key,
                    name: app.name,
                    icon: app.icon,
                    started: now,
                    accumulated: 0,
                    last_heartbeat: now,
                });
                self.publish_current();
            }
            Err(e) => {
                self.set_status(
                    TrackingStatus::Error,
                    format!("Could not start a session: {e}"),
                );
            }
        }
    }

    fn maybe_prune(&mut self) {
        let settings = self.handle.settings();
        if settings.retention_days <= 0 {
            return;
        }
        let today = Day::today();
        if self.last_prune_day == Some(today) {
            return;
        }
        self.last_prune_day = Some(today);
        let cutoff = today.add_days(-(settings.retention_days as i32)).start_ts();
        if let Err(e) = self.store.prune_before(cutoff) {
            eprintln!("daymark: could not apply retention policy: {e}");
        }
    }

    fn tick(&mut self) {
        if self.handle.reset.swap(false, Ordering::Relaxed) {
            if let Some(session) = &self.current {
                let end = session.started + session.accumulated;
                self.close_current(end, false);
            }
            if let Err(e) = self.store.delete_all() {
                self.set_status(TrackingStatus::Error, format!("Could not clear data: {e}"));
            }
            self.resolved.clear();
            self.previous = None;
            if let Ok(mut live) = self.handle.live.lock() {
                live.current = None;
                live.db_version += 1;
            }
        }
        let now_wall = date::unix_now();
        let now_mono = Instant::now();
        let mono_delta = now_mono.duration_since(self.last_tick_mono).as_secs() as i64;
        let wall_delta = now_wall - self.last_tick_wall;
        // A large wall-clock jump with a small monotonic delta means the
        // machine slept, or the clock was adjusted. Credit neither.
        let gap = (wall_delta - mono_delta).abs() > 5 || mono_delta > 10;
        self.last_tick_wall = now_wall;
        self.last_tick_mono = now_mono;

        let settings = self.handle.settings();
        if settings.paused {
            if settings.pause_expired(now_wall) {
                // A timed pause ends on its own, even with the window hidden
                // or the process in --background. Resume and persist so a
                // restart agrees; no time was recorded while paused.
                let mut resumed = settings.clone();
                resumed.paused = false;
                resumed.paused_until = 0;
                if let Err(e) = resumed.save(&self.store) {
                    self.set_status(
                        TrackingStatus::Error,
                        format!("Could not save settings: {e}"),
                    );
                }
                self.handle.set_settings(resumed);
                // Fall through: this tick resumes tracking normally.
            } else {
                if self.current.is_some() {
                    let end = self
                        .current
                        .as_ref()
                        .map(|s| s.started + s.accumulated)
                        .unwrap_or(now_wall);
                    self.close_current(end, false);
                }
                self.set_status(TrackingStatus::Paused, "Tracking is paused");
                return;
            }
        }

        if gap && self.current.is_some() {
            let end = self
                .current
                .as_ref()
                .map(|s| s.started + s.accumulated)
                .unwrap_or(now_wall);
            self.close_current(end, false);
        }

        if self.last_prune_day != Some(Day::today()) {
            self.maybe_prune();
        }

        let delta = if gap { 0 } else { mono_delta.clamp(0, 5) };

        let polled = self.reader.poll();
        let Some((state, age)) = polled else {
            if let Some(session) = &self.current {
                let end = session.started + session.accumulated;
                self.close_current(end, false);
            }
            let (status, detail) = if !self.capability.extension_installed {
                (
                    TrackingStatus::NeedsSetup,
                    "GNOME integration is not installed".to_string(),
                )
            } else if !self.capability.extension_enabled {
                (
                    TrackingStatus::NeedsSetup,
                    "GNOME integration is not enabled".to_string(),
                )
            } else if self.ever_saw_state {
                (
                    TrackingStatus::Stale,
                    "The GNOME integration stopped reporting".to_string(),
                )
            } else {
                (
                    TrackingStatus::Waiting,
                    "Sign out and back in once to activate tracking".to_string(),
                )
            };
            self.set_status(status, detail);
            return;
        };
        self.ever_saw_state = true;
        if age > 45 {
            if let Some(session) = &self.current {
                let end = session.started + session.accumulated;
                self.close_current(end, false);
            }
            self.set_status(
                TrackingStatus::Stale,
                "The GNOME integration stopped reporting",
            );
            return;
        }
        if state.away {
            if self.current.is_some() {
                let end = if state.away_since > 0 {
                    state.away_since.min(now_wall)
                } else {
                    self.current
                        .as_ref()
                        .map(|s| s.started + s.accumulated)
                        .unwrap_or(now_wall)
                };
                self.close_current(end, false);
            }
            let detail = if state.locked {
                "The screen is locked"
            } else if state.idle {
                "No input for a while"
            } else {
                "Away from the computer"
            };
            self.set_status(TrackingStatus::Away, detail);
            return;
        }
        let Some(focus) = state.focus else {
            if let Some(session) = &self.current {
                let end = session.started + session.accumulated;
                self.close_current(end, false);
            }
            self.set_status(TrackingStatus::Active, "No active window");
            return;
        };

        // Shell-owned surfaces (gjs, the desktop-icons process, the screen
        // shield) are not applications. The extension filters them too; this
        // guard covers an old state file or a future Shell regression.
        if crate::desktop::is_internal_window(focus.wm_class.as_deref(), focus.name.as_deref()) {
            if let Some(session) = &self.current {
                let end = session.started + session.accumulated;
                self.close_current(end, false);
            }
            self.set_status(TrackingStatus::Active, "No active window");
            return;
        }

        let app = self.resolve(&focus);
        let current_key = self.current.as_ref().map(|s| s.key.clone());
        let needs_start = current_key.as_deref() != Some(app.key.as_str());
        let crossed_midnight = self
            .current
            .as_ref()
            .is_some_and(|s| Day::from_timestamp(s.started) != Day::from_timestamp(now_wall));

        if needs_start || crossed_midnight {
            if let Some(session) = &self.current {
                let end = if crossed_midnight {
                    now_wall
                } else {
                    (session.started + session.accumulated + delta).min(now_wall)
                };
                self.close_current(end, !crossed_midnight);
            }
            if crossed_midnight || !self.try_resume_previous(&app, now_wall) {
                self.start_session(app.clone(), now_wall);
            }
        } else {
            let mut heartbeat = None;
            let published = if let Some(session) = &mut self.current {
                session.accumulated = session.accumulated.saturating_add(delta);
                let end = session.started.saturating_add(session.accumulated);
                if end - session.last_heartbeat >= 30 {
                    session.last_heartbeat = end;
                    heartbeat = Some((session.db_id, end));
                }
                Some(LiveCurrent {
                    app_id: session.app_id,
                    key: session.key.clone(),
                    name: session.name.clone(),
                    icon: session.icon.clone(),
                    started: session.started,
                    elapsed: session.accumulated,
                })
            } else {
                None
            };
            if let Some((db_id, end)) = heartbeat {
                if let Err(e) = self.store.heartbeat_session(db_id, end) {
                    self.set_status(
                        TrackingStatus::Error,
                        format!("Could not update usage: {e}"),
                    );
                }
            }
            if let Some(published) = published {
                if let Ok(mut live) = self.handle.live.lock() {
                    live.current = Some(published);
                    live.ever_saw_state = true;
                }
            }
        }
        let detail = format!("Tracking {}", app.name);
        self.set_status(TrackingStatus::Active, detail);
    }
}

pub fn spawn(store: Store, settings: Settings) -> TrackerHandle {
    let handle = TrackerHandle {
        live: Arc::new(Mutex::new(LiveState::default())),
        settings: Arc::new(Mutex::new(settings)),
        quit: Arc::new(AtomicBool::new(false)),
        reset: Arc::new(AtomicBool::new(false)),
        join: Arc::new(Mutex::new(None)),
    };
    let thread_handle = handle.clone();
    let join_slot = handle.join.clone();
    let thread = std::thread::Builder::new()
        .name("daymark-tracker".into())
        .spawn(move || {
            let mut tracker = Tracker::new(store, thread_handle.clone());
            while !thread_handle.quit.load(Ordering::Relaxed) {
                tracker.tick();
                // Two seconds is forgiving enough for input/idle transitions
                // while waking the CPU only 30 times a minute. `park_timeout`
                // is interrupted immediately by `stop()`/`request_reset()`.
                std::thread::park_timeout(Duration::from_secs(2));
            }
            if let Some(session) = &tracker.current {
                let end = session.started + session.accumulated;
                tracker.close_current(end, false);
            }
        })
        .expect("could not start the tracking thread");
    if let Ok(mut slot) = join_slot.lock() {
        *slot = Some(thread);
    }
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::SessionRow;

    #[test]
    fn parses_the_extension_state_file() {
        let text = r#"{"v":1,"seq":4,"stamp":1710000000000,"focus":{"id":"org.gnome.Nautilus.desktop","name":"Files","wm_class":"org.gnome.Nautilus","pid":12},"away":false,"away_since":0,"idle":false,"locked":false}"#;
        let state = parse_state(text).unwrap();
        assert_eq!(
            state.focus.as_ref().unwrap().id,
            "org.gnome.Nautilus.desktop"
        );
        assert!(!state.away);
    }

    #[test]
    fn accepts_a_null_focus_while_away() {
        let text = r#"{"v":1,"focus":null,"away":true,"away_since":1710000000,"idle":false,"locked":true}"#;
        let state = parse_state(text).unwrap();
        assert!(state.focus.is_none());
        assert!(state.away);
        assert!(state.locked);
    }

    #[test]
    fn normalises_millisecond_away_timestamps() {
        let text = r#"{"v":1,"focus":null,"away":true,"away_since":1710000000123,"idle":false,"locked":false}"#;
        assert_eq!(parse_state(text).unwrap().away_since, 1_710_000_000);
        let text = r#"{"v":2,"focus":null,"away":true,"away_since":1710000000,"idle":false,"locked":false}"#;
        assert_eq!(parse_state(text).unwrap().away_since, 1_710_000_000);
    }

    #[test]
    fn ignores_malformed_state() {
        assert!(parse_state("not json").is_none());
        assert!(parse_state("{}").is_none());
        assert!(parse_state(r#"{"v":0}"#).is_none());
    }

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daymark-tracker-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn state_path(&self) -> std::path::PathBuf {
            self.0.join("state.json")
        }
        fn db_path(&self) -> std::path::PathBuf {
            self.0.join("daymark.db")
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_state(
        path: &std::path::Path,
        seq: u32,
        focus: Option<(&str, &str)>,
        away: bool,
        away_since: i64,
    ) {
        let focus_json = match focus {
            Some((id, name)) => {
                format!(r#"{{"id":"{id}","name":"{name}","wm_class":null,"pid":null}}"#)
            }
            None => "null".to_string(),
        };
        let pad = " ".repeat(seq as usize % 7);
        let body = format!(
            r#"{{"v":1,"seq":{seq},"stamp":{},"focus":{focus_json},"away":{away},"away_since":{away_since},"idle":{away},"locked":false,"pad":"{pad}"}}"#,
            date::unix_now() * 1000
        );
        std::fs::write(path, body).unwrap();
    }

    fn make_tracker(dir: &TempDir) -> Tracker {
        let store = Store::open_at(&dir.db_path()).unwrap();
        let settings = Settings::default();
        let handle = TrackerHandle::demo(settings, TrackingStatus::Active, "test");
        Tracker {
            store,
            reader: StateReader::new(dir.state_path()),
            index: DesktopIndex::default(),
            index_loaded: Instant::now(),
            resolved: HashMap::new(),
            current: None,
            previous: None,
            // Legacy pace: these tests hold each focus for about one second
            // and expect it to be recorded. Newer behaviour tests set the
            // production floor explicitly.
            noise_floor: 1,
            last_tick_wall: date::unix_now(),
            last_tick_mono: Instant::now(),
            handle,
            last_prune_day: Some(Day::today()),
            capability: platform::Capabilities::default(),
            ever_saw_state: false,
            last_status: TrackingStatus::Active,
            last_detail: String::new(),
        }
    }

    fn rows(tracker: &Tracker) -> Vec<SessionRow> {
        tracker
            .store
            .sessions_between(0, date::unix_now() + 3600)
            .unwrap()
    }

    /// Same as `rows`, for tests that bind a local named `rows`.
    fn session_rows(tracker: &Tracker) -> Vec<SessionRow> {
        rows(tracker)
    }

    #[test]
    fn focus_changes_produce_non_overlapping_sessions() {
        let dir = TempDir::new("focus");
        let mut tracker = make_tracker(&dir);
        let t0 = date::unix_now();
        write_state(
            &dir.state_path(),
            1,
            Some(("org.example.Alpha", "Alpha")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        assert!(tracker.current.is_some());
        std::thread::sleep(Duration::from_millis(1100));
        tracker.tick();
        write_state(
            &dir.state_path(),
            2,
            Some(("org.example.Beta", "Beta")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        std::thread::sleep(Duration::from_millis(1100));
        tracker.tick();
        write_state(&dir.state_path(), 3, None, true, date::unix_now());
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        assert!(tracker.current.is_none());
        let rows = rows(&tracker);
        assert!(rows.len() >= 2, "rows: {rows:?}");
        let mut ordered = rows.clone();
        ordered.sort_by_key(|r| r.started);
        for pair in ordered.windows(2) {
            assert!(pair[1].started >= pair[0].ended, "overlap: {ordered:?}");
        }
        assert!(rows.iter().all(|r| r.ended > r.started));
        assert!((date::unix_now() - t0) < 10);
    }

    #[test]
    fn idle_timestamp_truncates_the_open_session() {
        let dir = TempDir::new("idle");
        let mut tracker = make_tracker(&dir);
        write_state(
            &dir.state_path(),
            1,
            Some(("org.example.Alpha", "Alpha")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        std::thread::sleep(Duration::from_millis(1200));
        tracker.tick();
        let start = tracker.current.as_ref().unwrap().started;
        // Idle is reported two seconds late, but the extension reports the
        // exact moment input stopped (one second into the session).
        write_state(
            &dir.state_path(),
            2,
            Some(("org.example.Alpha", "Alpha")),
            true,
            start + 1,
        );
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        assert!(tracker.current.is_none());
        let rows = rows(&tracker);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].started, start);
        assert_eq!(rows[0].ended, start + 1);
    }

    #[test]
    fn pause_records_nothing_and_resume_starts_cleanly() {
        let dir = TempDir::new("pause");
        let mut tracker = make_tracker(&dir);
        tracker.handle.settings.lock().unwrap().paused = true;
        write_state(
            &dir.state_path(),
            1,
            Some(("org.example.Alpha", "Alpha")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        assert!(tracker.current.is_none());
        assert_eq!(tracker.last_status, TrackingStatus::Paused);
        tracker.handle.settings.lock().unwrap().paused = false;
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        assert!(tracker.current.is_some());
    }

    #[test]
    fn a_timed_pause_records_nothing_and_resumes_at_its_deadline() {
        let dir = TempDir::new("timed-pause");
        let mut tracker = make_tracker(&dir);
        let deadline = date::unix_now() + 3600;
        {
            let mut settings = tracker.handle.settings.lock().unwrap();
            settings.paused = true;
            settings.paused_until = deadline;
        }
        write_state(
            &dir.state_path(),
            1,
            Some(("org.example.Alpha", "Alpha")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        // While the pause runs, a focused app records nothing.
        assert!(tracker.current.is_none());
        assert_eq!(tracker.last_status, TrackingStatus::Paused);
        assert!(rows(&tracker).is_empty());

        // After the deadline the tracker resumes on its own, clears the
        // persisted pause, and only then starts counting again.
        let expiry = date::unix_now() - 1;
        tracker.handle.settings.lock().unwrap().paused_until = expiry;
        std::thread::sleep(Duration::from_millis(30));
        tracker.tick();
        assert_eq!(tracker.last_status, TrackingStatus::Active);
        assert!(tracker.current.is_some());
        let resumed = tracker.handle.settings();
        assert!(!resumed.paused);
        assert_eq!(resumed.paused_until, 0);
        assert_eq!(tracker.store.get_setting("paused").as_deref(), Some("0"));
        assert_eq!(
            tracker.store.get_setting("paused_until").as_deref(),
            Some("0")
        );
        let started = tracker.current.as_ref().unwrap().started;
        assert!(
            started >= expiry && tracker.current.as_ref().unwrap().accumulated == 0,
            "no paused time may be recorded"
        );
    }

    /// A tracker with the production noise floor, for tests that exercise
    /// short-focus merging and resumption.
    fn make_floor_tracker(dir: &TempDir) -> Tracker {
        let mut tracker = make_tracker(dir);
        tracker.noise_floor = NOISE_FLOOR_SECS;
        tracker
    }

    fn app(key: &str) -> ResolvedApp {
        ResolvedApp {
            key: key.into(),
            name: key.into(),
            icon: None,
            desktop_id: None,
            wm_class: None,
            matched: false,
        }
    }

    /// Plants an already-closed session and records it as the tracker's
    /// most recent closed row.
    fn seed_closed(tracker: &mut Tracker, key: &str, started: i64, ended: i64, mergeable: bool) {
        let app = app(key);
        let app_id = tracker.store.upsert_app(&app, started).unwrap();
        let id = tracker.store.open_session(app_id, started).unwrap();
        tracker.store.close_session(id, ended).unwrap();
        tracker.previous = Some(PrevSession {
            db_id: id,
            app_id,
            key: app.key,
            started,
            ended,
            mergeable,
        });
    }

    fn open_fake(tracker: &mut Tracker, key: &str, started: i64) {
        let app = app(key);
        let app_id = tracker.store.upsert_app(&app, started).unwrap();
        let id = tracker.store.open_session(app_id, started).unwrap();
        tracker.current = Some(OpenSession {
            db_id: id,
            app_id,
            key: key.into(),
            name: key.into(),
            icon: None,
            started,
            accumulated: 0,
            last_heartbeat: started,
        });
    }

    #[test]
    fn a_short_blip_merges_into_the_preceding_session() {
        let dir = TempDir::new("blip");
        let mut tracker = make_floor_tracker(&dir);
        let now = date::unix_now();
        seed_closed(&mut tracker, "org.example.Alpha", now, now + 60, true);
        open_fake(&mut tracker, "org.example.Beta", now + 60);
        tracker.close_current(now + 63, true);
        let rows = session_rows(&tracker);
        assert_eq!(rows.len(), 1, "rows: {rows:?}");
        assert_eq!(rows[0].started, now);
        assert_eq!(rows[0].ended, now + 63);
        // A further short focus keeps folding into the same extended row.
        open_fake(&mut tracker, "org.example.Gamma", now + 63);
        tracker.close_current(now + 67, true);
        let rows = session_rows(&tracker);
        assert_eq!(rows.len(), 1, "rows: {rows:?}");
        assert_eq!(rows[0].ended, now + 67);
    }

    #[test]
    fn a_long_focus_keeps_its_own_row() {
        let dir = TempDir::new("long");
        let mut tracker = make_floor_tracker(&dir);
        let now = date::unix_now();
        seed_closed(&mut tracker, "org.example.Alpha", now, now + 60, true);
        open_fake(&mut tracker, "org.example.Beta", now + 60);
        tracker.close_current(now + 360, true);
        let rows = session_rows(&tracker);
        assert_eq!(rows.len(), 2, "rows: {rows:?}");
        assert_eq!(rows[0].ended, now + 60);
        assert_eq!(rows[1].started, now + 60);
        assert_eq!(rows[1].ended, now + 360);
    }

    #[test]
    fn a_gap_prevents_the_blip_from_becoming_or_merging_a_row() {
        let dir = TempDir::new("gap");
        let mut tracker = make_floor_tracker(&dir);
        let now = date::unix_now();
        seed_closed(&mut tracker, "org.example.Alpha", now, now + 60, true);
        // A minute of silence separates the closed row from the blip: not
        // contiguous, so the blip merges into nothing and is discarded.
        open_fake(&mut tracker, "org.example.Beta", now + 120);
        tracker.close_current(now + 123, true);
        let rows = session_rows(&tracker);
        assert_eq!(rows.len(), 1, "rows: {rows:?}");
        assert_eq!(rows[0].started, now);
        assert_eq!(rows[0].ended, now + 60);
        assert_eq!(tracker.store.session_count(), 1);
    }

    #[test]
    fn a_first_short_focus_makes_no_row_and_no_invented_previous_app() {
        let dir = TempDir::new("first-blip");
        let mut tracker = make_floor_tracker(&dir);
        let now = date::unix_now();
        open_fake(&mut tracker, "org.example.Beta", now);
        tracker.close_current(now + 3, true);
        assert_eq!(tracker.store.session_count(), 0);
        assert_eq!(session_rows(&tracker).len(), 0);
        assert!(tracker.previous.is_none());
    }

    #[test]
    fn a_blip_after_an_unmergeable_close_is_discarded() {
        let dir = TempDir::new("blocked");
        let mut tracker = make_floor_tracker(&dir);
        let now = date::unix_now();
        // mergeable is false whenever the preceding close came from a gap,
        // being away, a stale state file, or a pause.
        seed_closed(&mut tracker, "org.example.Alpha", now, now + 60, false);
        open_fake(&mut tracker, "org.example.Beta", now + 60);
        tracker.close_current(now + 63, true);
        let rows = session_rows(&tracker);
        assert_eq!(rows.len(), 1, "rows: {rows:?}");
        assert_eq!(rows[0].ended, now + 60, "the closed row stays untouched");
        assert_eq!(tracker.store.session_count(), 1);
    }

    #[test]
    fn the_noise_floor_never_merges_across_midnight() {
        let dir = TempDir::new("midnight");
        let mut tracker = make_floor_tracker(&dir);
        let midnight = Day::today().start_ts();
        // Ends one second before midnight; the blip starts at midnight. One
        // second apart, but on different days: no merge.
        seed_closed(
            &mut tracker,
            "org.example.Alpha",
            midnight - 60,
            midnight - 1,
            true,
        );
        open_fake(&mut tracker, "org.example.Beta", midnight);
        tracker.close_current(midnight + 2, true);
        let rows = session_rows(&tracker);
        assert_eq!(rows.len(), 1, "rows: {rows:?}");
        assert_eq!(rows[0].started, midnight - 60);
        assert_eq!(rows[0].ended, midnight - 1);
        assert_eq!(tracker.store.session_count(), 1);
    }

    #[test]
    fn returning_to_the_same_app_continues_its_row() {
        let dir = TempDir::new("resume");
        let mut tracker = make_floor_tracker(&dir);
        write_state(
            &dir.state_path(),
            1,
            Some(("org.example.Alpha", "Alpha")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(2300));
        tracker.tick();
        // Let Alpha accumulate past the noise floor.
        std::thread::sleep(Duration::from_millis(5300));
        tracker.tick();
        let first_started = tracker.current.as_ref().unwrap().started;
        let alpha_key = tracker.current.as_ref().unwrap().key.clone();
        // A blip: Beta holds focus briefly, so it must not become a row.
        write_state(
            &dir.state_path(),
            2,
            Some(("org.example.Beta", "Beta")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(2300));
        tracker.tick();
        assert_ne!(tracker.current.as_ref().unwrap().key, alpha_key);
        // Focus returns to Alpha: the previous row continues in place.
        write_state(
            &dir.state_path(),
            3,
            Some(("org.example.Alpha", "Alpha")),
            false,
            0,
        );
        std::thread::sleep(Duration::from_millis(2300));
        tracker.tick();
        let resumed = tracker.current.as_ref().unwrap();
        assert_eq!(resumed.key, alpha_key);
        assert_eq!(
            resumed.started, first_started,
            "the original Alpha row should continue, not restart"
        );
        // Away ends the continued row; the journal shows exactly one Alpha
        // row covering everything.
        write_state(&dir.state_path(), 4, None, true, date::unix_now());
        std::thread::sleep(Duration::from_millis(5300));
        tracker.tick();
        assert!(tracker.current.is_none());
        let rows = session_rows(&tracker);
        assert_eq!(rows.len(), 1, "rows: {rows:?}");
        assert!(rows[0].ended > rows[0].started);
        for pair in rows.windows(2) {
            assert!(pair[1].started >= pair[0].ended);
        }
    }
}
