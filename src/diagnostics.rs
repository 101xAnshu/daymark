//! Read-only diagnostics: the `doctor` report and the one-line `--status`
//! summary. Neither path migrates, recovers or writes the database, and both
//! work while the main application holds its own connection.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::{date, db, json, logging, platform, settings};

/// The companion heartbeats every 15 seconds; 45 seconds of silence is stale.
const STATE_FRESH_SECS: u64 = 45;

#[derive(Clone, Debug)]
struct Paths {
    db: PathBuf,
    state: PathBuf,
    /// The user-level companion written by source and archive installs.
    extension_dir: PathBuf,
    /// The read-only companion shipped by a package. It wins when present.
    system_extension_dir: PathBuf,
    autostart: PathBuf,
    logs: PathBuf,
}

impl Paths {
    fn current() -> Self {
        Self {
            db: db::db_path(),
            state: platform::state_path(),
            extension_dir: platform::extension_dir(),
            system_extension_dir: platform::system_extension_dir(),
            autostart: db::autostart_path(),
            logs: logging::log_dir(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenSession {
    app: String,
    started: i64,
}

#[derive(Clone, Debug)]
struct ExtensionState {
    focus_name: Option<String>,
    away: bool,
    idle: bool,
    locked: bool,
    heartbeat_age: Option<u64>,
}

#[derive(Clone, Debug)]
struct DoctorReport {
    db_path: PathBuf,
    db_error: Option<String>,
    schema_version: Option<i64>,
    db_size: u64,
    wal_size: u64,
    heartbeat_age: Option<u64>,
    state_present: bool,
    state_fresh: bool,
    open_session: Option<OpenSession>,
    rows_today: i64,
    extension_installed: bool,
    extension_enabled: bool,
    extension_version: Option<String>,
    autostart_installed: bool,
    retention_days: i64,
    last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pause {
    None,
    UntilResume,
    Until(i64),
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    match Connection::open_with_flags(path, flags) {
        Ok(conn) => {
            let _ = conn.busy_timeout(Duration::from_millis(500));
            let _ = conn.pragma_update(None, "query_only", "ON");
            Ok(conn)
        }
        Err(first) => {
            // A read-only WAL database can need a shared-memory file that is
            // absent after an unclean shutdown. Immutable mode reads the main
            // file without touching the WAL, and still never writes.
            let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI;
            Connection::open_with_flags(immutable_uri(path), flags).map_err(|second| {
                format!("could not open the database read-only: {first} ({second})")
            })
        }
    }
}

fn immutable_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut out = String::from("file:");
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out.push_str("?immutable=1");
    out
}

fn read_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .ok()
}

fn retention_days(conn: &Connection) -> i64 {
    read_setting(conn, "retention_days")
        .and_then(|value| value.parse::<i64>().ok())
        .map(|value| if value <= 0 { 0 } else { value.clamp(30, 3650) })
        .unwrap_or_else(|| settings::Settings::default().retention_days)
}

fn read_pause(conn: &Connection, now: i64) -> Pause {
    let paused = read_setting(conn, "paused")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !paused {
        return Pause::None;
    }
    let until = read_setting(conn, "paused_until")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    if until > 0 && now >= until {
        Pause::None
    } else if until > 0 {
        Pause::Until(until)
    } else {
        Pause::UntilResume
    }
}

fn query_schema_version(conn: &Connection) -> Result<i64, String> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("could not read the schema version: {error}"))
}

fn query_open_session(conn: &Connection) -> Result<Option<OpenSession>, String> {
    conn.query_row(
        "SELECT s.started_at, a.name FROM sessions s\n\
         JOIN apps a ON a.id = s.app_id\n\
         WHERE s.is_open = 1\n\
         ORDER BY s.started_at DESC LIMIT 1",
        [],
        |row| {
            Ok(OpenSession {
                started: row.get(0)?,
                app: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|error| format!("could not read the open session: {error}"))
}

fn query_rows_today(conn: &Connection, now: i64) -> Result<i64, String> {
    let day = date::Day::from_timestamp(now);
    conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE ended_at > ?1 AND started_at < ?2",
        params![day.start_ts(), day.end_ts()],
        |row| row.get(0),
    )
    .map_err(|error| format!("could not count today's rows: {error}"))
}

fn file_age_secs(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|age| age.as_secs())
}

fn parse_extension_state(text: &str, now: i64) -> Option<ExtensionState> {
    let value = json::parse(text)?;
    if value.get("v")?.as_i64()? < 1 {
        return None;
    }
    let focus_name = value.get("focus").and_then(|focus| {
        let id = focus.get("id")?.as_str()?;
        if id.is_empty() || id == "wmclass:" {
            return None;
        }
        let name = focus
            .get("name")
            .and_then(|name| name.as_str())
            .filter(|name| !name.is_empty());
        Some(name.unwrap_or(id).to_string())
    });
    let stamp = value
        .get("stamp")
        .and_then(|stamp| stamp.as_i64())
        .filter(|stamp| *stamp > 0)
        .map(|stamp| {
            if stamp > 10_000_000_000 {
                stamp / 1000
            } else {
                stamp
            }
        });
    Some(ExtensionState {
        focus_name,
        away: value.get("away").and_then(|v| v.as_bool()).unwrap_or(false),
        idle: value.get("idle").and_then(|v| v.as_bool()).unwrap_or(false),
        locked: value
            .get("locked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        heartbeat_age: stamp.map(|stamp| (now - stamp).max(0) as u64),
    })
}

fn state_snapshot(path: &Path, now: i64) -> (Option<u64>, Option<ExtensionState>) {
    let age = file_age_secs(path);
    let parsed = fs::read_to_string(path)
        .ok()
        .and_then(|text| parse_extension_state(&text, now));
    (age, parsed)
}

/// The directory of the companion GNOME will actually load, using the same
/// preference as `platform::extension_installed()`: the packaged system-wide
/// copy wins, then the user-level copy, and neither means "not installed".
fn selected_extension_dir(system_dir: &Path, user_dir: &Path) -> Option<PathBuf> {
    let source = platform::select_extension_source(
        system_dir.join("metadata.json").is_file(),
        user_dir.join("metadata.json").is_file(),
    );
    match source {
        platform::ExtensionSource::System => Some(system_dir.to_path_buf()),
        platform::ExtensionSource::User => Some(user_dir.to_path_buf()),
        platform::ExtensionSource::None => None,
    }
}

fn extension_version(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("metadata.json")).ok()?;
    let value = json::parse(&text)?;
    let version = value.get("version")?;
    version
        .as_i64()
        .map(|number| number.to_string())
        .or_else(|| version.as_str().map(str::to_string))
}

fn collect(
    paths: &Paths,
    now: i64,
    extension_enabled: bool,
    extension_installed: bool,
) -> DoctorReport {
    let db_size = fs::metadata(&paths.db).map(|meta| meta.len()).unwrap_or(0);
    let wal_size = fs::metadata(format!("{}-wal", paths.db.display()))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let (file_age, state) = state_snapshot(&paths.state, now);
    let state_present = file_age.is_some();
    let state_fresh = file_age.is_some_and(|age| age < STATE_FRESH_SECS);
    let heartbeat_age = state
        .as_ref()
        .and_then(|state| state.heartbeat_age)
        .or(file_age);
    let extension_dir = selected_extension_dir(&paths.system_extension_dir, &paths.extension_dir);

    let mut report = DoctorReport {
        db_path: paths.db.clone(),
        db_error: None,
        schema_version: None,
        db_size,
        wal_size,
        heartbeat_age,
        state_present,
        state_fresh,
        open_session: None,
        rows_today: 0,
        extension_installed,
        extension_enabled,
        extension_version: extension_dir.as_deref().and_then(extension_version),
        autostart_installed: paths.autostart.is_file(),
        retention_days: settings::Settings::default().retention_days,
        last_error: logging::last_error(&paths.logs),
    };

    if !paths.db.exists() {
        report.db_error = Some("database not found".into());
        return report;
    }
    let conn = match open_read_only(&paths.db) {
        Ok(conn) => conn,
        Err(error) => {
            report.db_error = Some(error);
            return report;
        }
    };
    match query_schema_version(&conn) {
        Ok(version) => report.schema_version = Some(version),
        Err(error) => report.db_error = Some(error),
    }
    report.retention_days = retention_days(&conn);
    match query_open_session(&conn) {
        Ok(session) => report.open_session = session,
        Err(error) => {
            if report.db_error.is_none() {
                report.db_error = Some(error);
            }
        }
    }
    match query_rows_today(&conn, now) {
        Ok(rows) => report.rows_today = rows,
        Err(error) => {
            if report.db_error.is_none() {
                report.db_error = Some(error);
            }
        }
    }
    report
}

fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn format_age(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds < 86_400 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3600)
    }
}

fn single_line(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

fn render(report: &DoctorReport) -> String {
    let state = if report.state_fresh {
        "fresh"
    } else if report.state_present {
        "stale"
    } else {
        "missing"
    };
    let session = match &report.open_session {
        Some(session) => format!(
            "{} (started {})",
            single_line(&session.app),
            logging::format_timestamp(session.started)
        ),
        None => "none".to_string(),
    };
    let mut out = String::new();
    out.push_str("Daymark doctor\n");
    out.push_str(&format!("database: {}\n", report.db_path.display()));
    if let Some(error) = &report.db_error {
        out.push_str(&format!("database error: {}\n", single_line(error)));
    }
    out.push_str(&format!(
        "schema version: {}\n",
        report
            .schema_version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
    out.push_str(&format!("database size: {}\n", human_bytes(report.db_size)));
    out.push_str(&format!("WAL size: {}\n", human_bytes(report.wal_size)));
    out.push_str(&format!(
        "heartbeat age: {}\n",
        report
            .heartbeat_age
            .map(format_age)
            .unwrap_or_else(|| "unknown".to_string())
    ));
    out.push_str(&format!("extension state: {state}\n"));
    out.push_str(&format!("open session: {session}\n"));
    out.push_str(&format!("rows recorded today: {}\n", report.rows_today));
    out.push_str(&format!(
        "extension installed: {}\n",
        if report.extension_installed {
            "yes"
        } else {
            "no"
        }
    ));
    out.push_str(&format!(
        "extension enabled: {}\n",
        if report.extension_enabled {
            "yes"
        } else {
            "no"
        }
    ));
    out.push_str(&format!(
        "extension version: {}\n",
        report.extension_version.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&format!(
        "autostart installed: {}\n",
        if report.autostart_installed {
            "yes"
        } else {
            "no"
        }
    ));
    out.push_str(&format!("retention days: {}\n", report.retention_days));
    out.push_str(&format!(
        "last error: {}\n",
        report
            .last_error
            .as_deref()
            .map(single_line)
            .unwrap_or_else(|| "none".to_string())
    ));
    out
}

fn pause_token(pause: Pause) -> String {
    match pause {
        Pause::None => "none".into(),
        Pause::UntilResume => "until-resume".into(),
        Pause::Until(at) => format!("until:{at}"),
    }
}

fn status_value(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn status_line(paths: &Paths, now: i64) -> String {
    let pause = open_read_only(&paths.db)
        .map(|conn| read_pause(&conn, now))
        .unwrap_or(Pause::None);
    let paused = pause != Pause::None;
    let (file_age, state) = state_snapshot(&paths.state, now);
    let state_fresh = file_age.is_some_and(|age| age < STATE_FRESH_SECS);
    let tracking = if paused {
        "paused"
    } else {
        match (file_age, state.as_ref()) {
            (None, _) => "waiting",
            (Some(_), None) => "stale",
            (Some(age), Some(_)) if age >= STATE_FRESH_SECS => "stale",
            (Some(_), Some(state)) if state.away || state.idle || state.locked => "away",
            (Some(_), Some(_)) => "active",
        }
    };
    let mut line = format!("tracking={tracking} pause={}", pause_token(pause));
    if state_fresh {
        if let Some(name) = state.as_ref().and_then(|state| state.focus_name.as_deref()) {
            line.push_str(&format!(" app={}", status_value(name)));
        }
    }
    line
}

pub fn doctor_output() -> String {
    let report = collect(
        &Paths::current(),
        date::unix_now(),
        platform::extension_enabled(),
        platform::extension_installed(),
    );
    render(&report)
}

pub fn status_output() -> String {
    status_line(&Paths::current(), date::unix_now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Day;
    use crate::db::Store;
    use crate::desktop::ResolvedApp;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daymark-diagnostics-{tag}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn paths(&self) -> Paths {
            Paths {
                db: self.0.join("daymark.db"),
                state: self.0.join("state.json"),
                extension_dir: self.0.join("extension"),
                system_extension_dir: self.0.join("system-extension"),
                autostart: self.0.join("autostart/daymark.desktop"),
                logs: self.0.join("logs"),
            }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn app() -> ResolvedApp {
        ResolvedApp {
            key: "org.example.Editor".into(),
            name: "Example Editor".into(),
            icon: None,
            desktop_id: Some("org.example.Editor.desktop".into()),
            wm_class: Some("Example Editor".into()),
            matched: true,
        }
    }

    fn seed_session(db_path: &Path, now: i64) -> Store {
        let store = Store::open_at(db_path).unwrap();
        let today = Day::from_timestamp(now);
        let id = store.upsert_app(&app(), now).unwrap();
        let closed = store.open_session(id, today.start_ts() + 60).unwrap();
        store.close_session(closed, today.start_ts() + 120).unwrap();
        store.open_session(id, today.start_ts() + 180).unwrap();
        // Keep the write connection open: doctor and status must work while
        // the application already holds its own connection and WAL.
        store
    }

    fn write_state(path: &Path, name: &str, away: bool, now: i64) {
        let body = format!(
            r#"{{"v":2,"seq":1,"stamp":{},"focus":{{"id":"org.example.Editor.desktop","name":"{name}"}},"away":{away},"away_since":0,"idle":{away},"locked":false}}"#,
            now * 1000
        );
        fs::write(path, body).unwrap();
    }

    #[test]
    fn doctor_renders_every_field_from_a_temp_database() {
        let dir = TempDir::new("doctor");
        let paths = dir.paths();
        let now = date::unix_now();
        fs::create_dir_all(&paths.extension_dir).unwrap();
        fs::write(
            paths.extension_dir.join("metadata.json"),
            r#"{"uuid":"daymark@daymark.app","version":2}"#,
        )
        .unwrap();
        let store = Store::open_at(&paths.db).unwrap();
        store.set_setting("retention_days", "90").unwrap();
        crate::db::migrate_legacy_identities(&store);
        drop(store);
        let _live = seed_session(&paths.db, now);
        write_state(&paths.state, "Example Editor", false, now);
        fs::create_dir_all(paths.autostart.parent().unwrap()).unwrap();
        fs::write(&paths.autostart, "[Desktop Entry]\n").unwrap();
        fs::create_dir_all(&paths.logs).unwrap();
        fs::write(
            paths
                .logs
                .join(format!("daymark-{}.log", Day::from_timestamp(now).iso())),
            "2026-10-26 14:00:00 level=ERROR event=save message=Could not save\n",
        )
        .unwrap();

        let report = collect(&paths, now, false, true);
        assert_eq!(report.db_error, None);
        assert_eq!(report.schema_version, Some(3));
        assert_eq!(report.rows_today, 2);
        assert_eq!(report.open_session.as_ref().unwrap().app, "Example Editor");
        assert!(report.state_present);
        assert!(report.state_fresh);
        assert_eq!(report.extension_version.as_deref(), Some("2"));
        assert!(report.extension_installed);
        assert!(!report.extension_enabled);
        assert!(report.autostart_installed);
        assert_eq!(report.retention_days, 90);
        assert!(
            report
                .last_error
                .as_deref()
                .unwrap()
                .ends_with("event=save message=Could not save")
        );

        let output = render(&report);
        assert!(output.starts_with("Daymark doctor\n"));
        assert!(output.contains(&format!("database: {}\n", paths.db.display())));
        assert!(output.contains("\nschema version: 3\n"));
        assert!(output.contains("\ndatabase size: "));
        assert!(output.contains("\nWAL size: "));
        assert!(output.contains("\nheartbeat age: "));
        assert!(output.contains("\nextension state: fresh\n"));
        assert!(output.contains("\nopen session: Example Editor (started "));
        assert!(output.contains("\nrows recorded today: 2\n"));
        assert!(output.contains("\nextension installed: yes\n"));
        assert!(output.contains("\nextension enabled: no\n"));
        assert!(output.contains("\nextension version: 2\n"));
        assert!(output.contains("\nautostart installed: yes\n"));
        assert!(output.contains("\nretention days: 90\n"));
        assert!(
            output
                .contains("\nlast error: 2026-10-26 14:00:00 event=save message=Could not save\n")
        );
    }

    #[test]
    fn doctor_prefers_the_packaged_system_extension() {
        let dir = TempDir::new("doctor-system-extension");
        let paths = dir.paths();
        fs::create_dir_all(&paths.system_extension_dir).unwrap();
        fs::write(
            paths.system_extension_dir.join("metadata.json"),
            r#"{"uuid":"daymark@daymark.app","version":3}"#,
        )
        .unwrap();
        fs::create_dir_all(&paths.extension_dir).unwrap();
        fs::write(
            paths.extension_dir.join("metadata.json"),
            r#"{"uuid":"daymark@daymark.app","version":2}"#,
        )
        .unwrap();

        let report = collect(&paths, date::unix_now(), true, true);
        assert!(report.extension_installed);
        assert_eq!(report.extension_version.as_deref(), Some("3"));
        let output = render(&report);
        assert!(output.contains("\nextension installed: yes\n"));
        assert!(output.contains("\nextension version: 3\n"));
    }

    #[test]
    fn doctor_finds_a_system_extension_without_a_user_copy() {
        let dir = TempDir::new("doctor-system-only");
        let paths = dir.paths();
        fs::create_dir_all(&paths.system_extension_dir).unwrap();
        fs::write(
            paths.system_extension_dir.join("metadata.json"),
            r#"{"uuid":"daymark@daymark.app","version":"4"}"#,
        )
        .unwrap();

        let report = collect(&paths, date::unix_now(), true, true);
        assert!(report.extension_installed);
        assert_eq!(report.extension_version.as_deref(), Some("4"));
    }

    #[test]
    fn doctor_reports_a_missing_database_without_creating_it() {
        let dir = TempDir::new("missing");
        let paths = dir.paths();
        let report = collect(&paths, date::unix_now(), false, false);
        assert_eq!(report.db_error.as_deref(), Some("database not found"));
        assert_eq!(report.schema_version, None);
        assert_eq!(report.open_session, None);
        assert_eq!(report.rows_today, 0);
        assert!(!paths.db.exists());
        let output = render(&report);
        assert!(output.contains("\ndatabase error: database not found\n"));
        assert!(output.contains("\nlast error: none\n"));
    }

    #[test]
    fn status_reports_an_active_fresh_app() {
        let dir = TempDir::new("status-active");
        let paths = dir.paths();
        let now = date::unix_now();
        let _live = seed_session(&paths.db, now);
        write_state(&paths.state, "Example Editor", false, now);
        assert_eq!(
            status_line(&paths, now),
            "tracking=active pause=none app=\"Example Editor\""
        );
    }

    #[test]
    fn status_reports_a_timed_pause_and_its_deadline() {
        let dir = TempDir::new("status-paused");
        let paths = dir.paths();
        let now = date::unix_now();
        let until = now + 600;
        {
            let store = Store::open_at(&paths.db).unwrap();
            store.set_setting("paused", "1").unwrap();
            store
                .set_setting("paused_until", &until.to_string())
                .unwrap();
        }
        write_state(&paths.state, "Example Editor", false, now);
        assert_eq!(
            status_line(&paths, now),
            format!("tracking=paused pause=until:{until} app=\"Example Editor\"")
        );
        // A deadline in the past is no longer a pause.
        assert_eq!(
            status_line(&paths, until + 1),
            "tracking=active pause=none app=\"Example Editor\""
        );
    }

    #[test]
    fn status_marks_an_old_state_file_as_stale_and_omits_the_app() {
        let dir = TempDir::new("status-stale");
        let paths = dir.paths();
        let now = date::unix_now();
        {
            let _store = Store::open_at(&paths.db).unwrap();
        }
        write_state(&paths.state, "Example Editor", false, now);
        let old = std::time::SystemTime::now() - Duration::from_secs(120);
        let file = fs::File::options().write(true).open(&paths.state).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
        assert_eq!(status_line(&paths, now), "tracking=stale pause=none");
    }
}
