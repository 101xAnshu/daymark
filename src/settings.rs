//! Small, explicit settings model. Everything is stored in the local SQLite
//! database except the runtime configuration handed to the GNOME extension.

use crate::db::Store;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePref {
    System,
    Light,
    Dark,
}

impl ThemePref {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "light" => ThemePref::Light,
            "dark" => ThemePref::Dark,
            _ => ThemePref::System,
        }
    }
    pub fn index(self) -> i32 {
        match self {
            ThemePref::System => 0,
            ThemePref::Light => 1,
            ThemePref::Dark => 2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    /// Seconds without input before tracking pauses. Clamped to 30..=3600.
    pub idle_seconds: i64,
    pub paused: bool,
    /// Unix timestamp (seconds) when a timed pause ends. 0 means the pause
    /// has no deadline ("until resumed") or tracking is not paused.
    pub paused_until: i64,
    pub autostart: bool,
    /// 0 means keep everything.
    pub retention_days: i64,
    pub theme: ThemePref,
    /// The first-run setup sheet opens only once. False on a fresh install
    /// and after the sheet has been shown (or was not needed) on the first
    /// visible launch.
    pub first_run_complete: bool,
    /// Send one local notification a week with the previous week's totals.
    pub digest_enabled: bool,
    /// Start timestamp of the last week end delivered, or 0 when never sent.
    pub digest_last_sent: i64,
    /// Read browser history read-only to show domain-only site names. Off by
    /// default; the V1 privacy promise is unchanged until this is enabled.
    pub site_names_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_seconds: 300,
            paused: false,
            paused_until: 0,
            autostart: true,
            retention_days: 365,
            theme: ThemePref::System,
            first_run_complete: false,
            digest_enabled: false,
            digest_last_sent: 0,
            site_names_enabled: false,
        }
    }
}

impl Settings {
    pub fn load(store: &Store) -> Self {
        let mut s = Settings::default();
        if let Some(v) = store
            .get_setting("idle_seconds")
            .and_then(|v| v.parse::<i64>().ok())
        {
            s.idle_seconds = v.clamp(30, 3600);
        }
        if let Some(v) = store.get_setting("paused") {
            s.paused = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Some(v) = store
            .get_setting("paused_until")
            .and_then(|v| v.parse::<i64>().ok())
        {
            s.paused_until = v.max(0);
        }
        s.resolve_pause_at_start(store);
        if let Some(v) = store.get_setting("autostart") {
            s.autostart = v != "0" && !v.eq_ignore_ascii_case("false");
        }
        if let Some(v) = store
            .get_setting("retention_days")
            .and_then(|v| v.parse::<i64>().ok())
        {
            s.retention_days = if v <= 0 { 0 } else { v.clamp(30, 3650) };
        }
        if let Some(v) = store.get_setting("theme") {
            s.theme = ThemePref::parse(&v);
        }
        if let Some(v) = store.get_setting("first_run_complete") {
            s.first_run_complete = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Some(v) = store.get_setting("digest_enabled") {
            s.digest_enabled = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Some(v) = store
            .get_setting("digest_last_sent")
            .and_then(|v| v.parse::<i64>().ok())
        {
            s.digest_last_sent = v.max(0);
        }
        if let Some(v) = store.get_setting("site_names_enabled") {
            s.site_names_enabled = v == "1" || v.eq_ignore_ascii_case("true");
        }
        s
    }

    /// A pause never silently outlives a restart: an open-ended pause
    /// (including the old `paused=1` format without a deadline) resumes at
    /// the next app start, and so does a timed pause whose deadline passed
    /// while the app was off. Only a timed pause that has not expired yet
    /// stays paused. The cleared state is written back immediately so the
    /// next start agrees.
    fn resolve_pause_at_start(&mut self, store: &Store) {
        if !self.paused {
            self.paused_until = 0;
            return;
        }
        let expired = self.paused_until > 0 && crate::date::unix_now() >= self.paused_until;
        if self.paused_until == 0 || expired {
            self.paused = false;
            self.paused_until = 0;
            let _ = store.set_setting("paused", "0");
            let _ = store.set_setting("paused_until", "0");
        }
    }

    /// True when a timed pause has reached its deadline and tracking should
    /// resume on its own.
    pub fn pause_expired(&self, now: i64) -> bool {
        self.paused && self.paused_until > 0 && now >= self.paused_until
    }

    pub fn save(&self, store: &Store) -> Result<(), String> {
        store
            .set_setting("idle_seconds", &self.idle_seconds.to_string())
            .and_then(|_| store.set_setting("paused", if self.paused { "1" } else { "0" }))
            .and_then(|_| store.set_setting("paused_until", &self.paused_until.to_string()))
            .and_then(|_| store.set_setting("autostart", if self.autostart { "1" } else { "0" }))
            .and_then(|_| store.set_setting("retention_days", &self.retention_days.to_string()))
            .and_then(|_| store.set_setting("theme", self.theme.as_str()))
            .and_then(|_| {
                store.set_setting(
                    "first_run_complete",
                    if self.first_run_complete { "1" } else { "0" },
                )
            })
            .and_then(|_| {
                store.set_setting(
                    "digest_enabled",
                    if self.digest_enabled { "1" } else { "0" },
                )
            })
            .and_then(|_| store.set_setting("digest_last_sent", &self.digest_last_sent.to_string()))
            .and_then(|_| {
                store.set_setting(
                    "site_names_enabled",
                    if self.site_names_enabled { "1" } else { "0" },
                )
            })
    }
}

/// Decide whether the first launch should open the setup sheet. True only
/// on a first run while tracking is not demonstrably working: the
/// companion must be installed, enabled, and its state file fresh. The
/// outcome is remembered (`first_run_complete`) so the sheet opens once and
/// never nags afterwards. Pure, so tests can cover the decision without a
/// live GNOME session.
pub fn first_run_needed(
    first_run_complete: bool,
    extension_installed: bool,
    extension_enabled: bool,
    state_fresh: bool,
) -> bool {
    !first_run_complete && !(extension_installed && extension_enabled && state_fresh)
}

/// Runtime config consumed by the GNOME extension so its idle watch follows
/// the same threshold as the interface.
pub fn write_extension_config(runtime_dir: &std::path::Path, idle_seconds: i64) {
    let _ = std::fs::create_dir_all(runtime_dir);
    let path = runtime_dir.join("config.json");
    let body = format!(
        "{{\"idle_seconds\":{},\"version\":1}}\n",
        idle_seconds.clamp(30, 3600)
    );
    let tmp = runtime_dir.join("config.json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDb(PathBuf);
    impl TempDb {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daymark-settings-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn db(&self) -> PathBuf {
            self.0.join("daymark.db")
        }
    }
    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Simulate a restart: open the database, set raw pause keys, then load.
    fn load_after(raw_paused: &str, raw_until: Option<i64>) -> (Settings, Store) {
        let dir = TempDb::new("pause");
        let store = Store::open_at(&dir.db()).unwrap();
        store.set_setting("paused", raw_paused).unwrap();
        if let Some(until) = raw_until {
            store
                .set_setting("paused_until", &until.to_string())
                .unwrap();
        }
        (Settings::load(&store), store)
    }

    #[test]
    fn an_indefinite_pause_resumes_at_the_next_start() {
        // Covers the new "until resumed" pause and the old paused=1 format:
        // neither carries a deadline, so neither survives a restart.
        let (settings, store) = load_after("1", None);
        assert!(!settings.paused);
        assert_eq!(settings.paused_until, 0);
        assert_eq!(store.get_setting("paused").as_deref(), Some("0"));
        assert_eq!(store.get_setting("paused_until").as_deref(), Some("0"));
    }

    #[test]
    fn an_active_timed_pause_survives_a_restart() {
        let until = crate::date::unix_now() + 900;
        let (settings, store) = load_after("1", Some(until));
        assert!(settings.paused);
        assert_eq!(settings.paused_until, until);
        // Untouched: still armed for the same deadline.
        assert_eq!(
            store.get_setting("paused_until").as_deref(),
            Some(until.to_string().as_str())
        );
    }

    #[test]
    fn an_expired_timed_pause_resumes_at_the_next_start() {
        let until = crate::date::unix_now() - 900;
        let (settings, store) = load_after("1", Some(until));
        assert!(!settings.paused);
        assert_eq!(settings.paused_until, 0);
        assert_eq!(store.get_setting("paused").as_deref(), Some("0"));
        assert_eq!(store.get_setting("paused_until").as_deref(), Some("0"));
    }

    #[test]
    fn an_unpaused_store_stays_unpaused() {
        let (settings, _store) = load_after("0", None);
        assert!(!settings.paused);
        assert_eq!(settings.paused_until, 0);
    }

    #[test]
    fn pause_expiry_matches_the_deadline() {
        let mut settings = Settings::default();
        assert!(!settings.pause_expired(0), "not paused");
        settings.paused = true;
        assert!(!settings.pause_expired(60), "no deadline is never expired");
        settings.paused_until = 120;
        assert!(!settings.pause_expired(119));
        assert!(settings.pause_expired(120));
        assert!(settings.pause_expired(121));
    }

    #[test]
    fn the_first_run_sheet_opens_while_tracking_is_not_working() {
        // Fresh install: nothing set up yet.
        assert!(first_run_needed(false, false, false, false));
        // Installed and enabled, but the extension has not run since the
        // login that loads it: the one re-login still needs explaining.
        assert!(first_run_needed(false, true, true, false));
        // Everything works: no sheet, just record completion.
        assert!(!first_run_needed(false, true, true, true));
        // Afterwards: never nag again, whatever the state.
        assert!(!first_run_needed(true, false, false, false));
        assert!(!first_run_needed(true, true, true, false));
    }

    #[test]
    fn the_first_run_marker_round_trips_through_the_store() {
        let dir = TempDb::new("first-run");
        let store = Store::open_at(&dir.db()).unwrap();

        let fresh = Settings::load(&store);
        assert!(!fresh.first_run_complete, "a fresh install runs first-run");

        let done = Settings {
            first_run_complete: true,
            ..Settings::default()
        };
        done.save(&store).unwrap();

        let reloaded = Settings::load(&store);
        assert!(reloaded.first_run_complete);
        assert_eq!(
            store.get_setting("first_run_complete").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn the_weekly_digest_preference_round_trips() {
        let dir = TempDb::new("digest");
        let store = Store::open_at(&dir.db()).unwrap();
        let fresh = Settings::load(&store);
        assert!(!fresh.digest_enabled, "the digest is opt-in");
        assert_eq!(fresh.digest_last_sent, 0);

        let sent = crate::date::Day::today().start_ts();
        let enabled = Settings {
            digest_enabled: true,
            digest_last_sent: sent,
            ..Settings::default()
        };
        enabled.save(&store).unwrap();

        let reloaded = Settings::load(&store);
        assert!(reloaded.digest_enabled);
        assert_eq!(reloaded.digest_last_sent, sent);
    }

    #[test]
    fn site_names_are_opt_in_and_round_trip() {
        let dir = TempDb::new("sites");
        let store = Store::open_at(&dir.db()).unwrap();
        assert!(!Settings::load(&store).site_names_enabled);

        let enabled = Settings {
            site_names_enabled: true,
            ..Settings::default()
        };
        enabled.save(&store).unwrap();
        assert!(Settings::load(&store).site_names_enabled);
    }
}
