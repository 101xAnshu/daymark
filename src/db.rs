//! Local SQLite storage. One small schema, versioned with `user_version`, with
//! WAL enabled so the interface can read while the tracker writes.

use crate::desktop::ResolvedApp;
use crate::stats::SessionRow;
use rusqlite::{Connection, params, params_from_iter};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Debug)]
pub struct AppInfo {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub icon: Option<String>,
    pub desktop_id: Option<String>,
}

pub struct Store {
    pub conn: Connection,
    pub path: PathBuf,
    pub recovered_corrupt_file: bool,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
        .join("daymark")
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("daymark")
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/daymark-{}", unsafe { libc::getuid() })))
        .join("daymark")
}

pub fn db_path() -> PathBuf {
    data_dir().join("daymark.db")
}

pub fn autostart_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("autostart/daymark.desktop")
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("could not create {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn looks_corrupt(err: &rusqlite::Error) -> bool {
    let text = err.to_string().to_lowercase();
    text.contains("not a database")
        || text.contains("malformed")
        || text.contains("corrupt")
        || text.contains("encrypted")
}

fn rename_corrupt(db: &Path) {
    let stamp = crate::date::unix_now();
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", db.display()));
        if from.exists() {
            let to = PathBuf::from(format!("{}.corrupt-{stamp}{suffix}", db.display()));
            let _ = fs::rename(from, to);
        }
    }
}

impl Store {
    pub fn open_default() -> Result<Self, String> {
        let dir = data_dir();
        ensure_private_dir(&dir)?;
        Self::open_at(&dir.join("daymark.db"))
    }

    pub fn open_at(db: &Path) -> Result<Self, String> {
        if let Some(parent) = db.parent() {
            let _ = ensure_private_dir(parent);
        }
        let mut recovered = false;
        let conn = match Connection::open(db) {
            Ok(c) => c,
            Err(e) if looks_corrupt(&e) => {
                rename_corrupt(db);
                recovered = true;
                Connection::open(db).map_err(|e| format!("could not open a fresh database: {e}"))?
            }
            Err(e) => return Err(format!("could not open the database: {e}")),
        };

        if let Err(e) = conn.execute_batch(
            "PRAGMA journal_mode = WAL;\n\
             PRAGMA synchronous = NORMAL;\n\
             PRAGMA foreign_keys = ON;\n\
             PRAGMA busy_timeout = 3000;\n\
             PRAGMA temp_store = MEMORY;",
        ) {
            if looks_corrupt(&e) {
                drop(conn);
                rename_corrupt(db);
                return Self::open_at(db).map(|mut s| {
                    s.recovered_corrupt_file = true;
                    s
                });
            }
            return Err(format!("could not configure the database: {e}"));
        }

        let check: Result<String, _> = conn.query_row("PRAGMA quick_check", [], |row| row.get(0));
        if let Ok(result) = &check {
            if !result.eq_ignore_ascii_case("ok") {
                drop(conn);
                rename_corrupt(db);
                return Self::open_at(db).map(|mut s| {
                    s.recovered_corrupt_file = true;
                    s
                });
            }
        }

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| format!("could not read the schema version: {e}"))?;
        if version > SCHEMA_VERSION {
            return Err(
                "This database was written by a newer version of Daymark. Please update Daymark."
                    .into(),
            );
        }
        if version < 1 {
            conn.execute_batch(
                "BEGIN;\n\
                 CREATE TABLE IF NOT EXISTS apps (\n\
                   id INTEGER PRIMARY KEY,\n\
                   key TEXT NOT NULL UNIQUE,\n\
                   name TEXT NOT NULL,\n\
                   icon_ref TEXT,\n\
                   desktop_id TEXT,\n\
                   wm_class TEXT,\n\
                   first_seen INTEGER NOT NULL,\n\
                   last_seen INTEGER NOT NULL\n\
                 );\n\
                 CREATE TABLE IF NOT EXISTS sessions (\n\
                   id INTEGER PRIMARY KEY,\n\
                   app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,\n\
                   started_at INTEGER NOT NULL,\n\
                   ended_at INTEGER NOT NULL,\n\
                   is_open INTEGER NOT NULL DEFAULT 0,\n\
                   source TEXT NOT NULL DEFAULT 'focus',\n\
                   CHECK(ended_at >= started_at)\n\
                 );\n\
                 CREATE INDEX IF NOT EXISTS sessions_started_idx ON sessions(started_at);\n\
                 CREATE INDEX IF NOT EXISTS sessions_ended_idx ON sessions(ended_at);\n\
                 CREATE INDEX IF NOT EXISTS sessions_open_idx ON sessions(is_open);\n\
                 CREATE UNIQUE INDEX IF NOT EXISTS sessions_one_open_idx ON sessions(is_open) WHERE is_open = 1;\n\
                 CREATE TABLE IF NOT EXISTS settings (\n\
                   key TEXT PRIMARY KEY,\n\
                   value TEXT NOT NULL\n\
                 );\n\
                 PRAGMA user_version = 1;\n\
                 COMMIT;",
            )
            .map_err(|e| format!("could not create the database schema: {e}"))?;
        }

        // A crash can leave one open interval. Its end timestamp is the last
        // heartbeat, so the sleeping gap is never credited to the application.
        conn.execute("UPDATE sessions SET is_open = 0 WHERE is_open = 1", [])
            .map_err(|e| format!("could not recover the last session: {e}"))?;

        Ok(Self {
            conn,
            path: db.to_path_buf(),
            recovered_corrupt_file: recovered,
        })
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2)\n\
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn upsert_app(&self, app: &ResolvedApp, now: i64) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO apps(key, name, icon_ref, desktop_id, wm_class, first_seen, last_seen)\n\
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6)\n\
                 ON CONFLICT(key) DO UPDATE SET\n\
                   name = excluded.name,\n\
                   icon_ref = excluded.icon_ref,\n\
                   desktop_id = excluded.desktop_id,\n\
                   wm_class = excluded.wm_class,\n\
                   last_seen = excluded.last_seen",
                params![
                    app.key,
                    app.name,
                    app.icon,
                    app.desktop_id,
                    app.wm_class,
                    now
                ],
            )
            .map_err(|e| format!("could not store the application: {e}"))?;
        self.conn
            .query_row("SELECT id FROM apps WHERE key = ?1", [&app.key], |r| {
                r.get(0)
            })
            .map_err(|e| format!("could not read the application: {e}"))
    }

    pub fn open_session(&self, app_id: i64, start: i64) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO sessions(app_id, started_at, ended_at, is_open)\n\
                 VALUES(?1, ?2, ?2, 1)",
                params![app_id, start],
            )
            .map_err(|e| format!("could not start a session: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn heartbeat_session(&self, id: i64, end: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE sessions SET ended_at = MAX(ended_at, ?1) WHERE id = ?2 AND is_open = 1",
                params![end, id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn close_session(&self, id: i64, end: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE sessions SET ended_at = MAX(started_at, ?1), is_open = 0\n\
                 WHERE id = ?2 AND is_open = 1",
                params![end, id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn discard_session(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1 AND is_open = 1", [id])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Re-opens a closed session in place. Used when focus returns to the same
    /// application immediately after a short-lived interruption, so the
    /// journal keeps one row instead of two adjacent ones. Fails while any
    /// other row is open, which keeps the at-most-one-open invariant.
    pub fn reopen_session(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE sessions SET is_open = 1 WHERE id = ?1 AND is_open = 0",
                [id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Folds a short focus session into the preceding application's row.
    ///
    /// The short row must still be open and the target must already be
    /// closed. One transaction so a failure can never leave the short row
    /// deleted while the target stays unextended.
    pub fn merge_short_session(
        &self,
        short_id: i64,
        target_id: i64,
        end: i64,
    ) -> Result<(), String> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;
        let outcome = (|| -> Result<(), String> {
            self.conn
                .execute(
                    "DELETE FROM sessions WHERE id = ?1 AND is_open = 1",
                    params![short_id],
                )
                .map_err(|e| e.to_string())?;
            self.conn
                .execute(
                    "UPDATE sessions SET ended_at = MAX(ended_at, ?2)\n\
                     WHERE id = ?1 AND is_open = 0",
                    params![target_id, end],
                )
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        match outcome {
            Ok(()) => self.conn.execute_batch("COMMIT").map_err(|e| e.to_string()),
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Sessions that contribute to a day. The currently open interval is
    /// excluded; the interface adds the live interval from the tracker.
    pub fn sessions_between(&self, start: i64, end: i64) -> Result<Vec<SessionRow>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT app_id, started_at, ended_at FROM sessions\n\
                 WHERE is_open = 0 AND ended_at > ?1 AND started_at < ?2\n\
                 ORDER BY started_at",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![start, end], |r| {
                Ok(SessionRow {
                    app_id: r.get(0)?,
                    started: r.get(1)?,
                    ended: r.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    pub fn app_infos(&self, ids: &[i64]) -> HashMap<i64, AppInfo> {
        if ids.is_empty() {
            return HashMap::new();
        }
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, key, name, icon_ref, desktop_id FROM apps WHERE id IN ({placeholders})"
        );
        let Ok(mut stmt) = self.conn.prepare(&sql) else {
            return HashMap::new();
        };
        let rows = stmt.query_map(params_from_iter(ids.iter()), |r| {
            Ok(AppInfo {
                id: r.get(0)?,
                key: r.get(1)?,
                name: r.get(2)?,
                icon: r.get(3)?,
                desktop_id: r.get(4)?,
            })
        });
        let mut out = HashMap::new();
        if let Ok(rows) = rows {
            for info in rows.flatten() {
                out.insert(info.id, info);
            }
        }
        out
    }

    pub fn has_any_sessions(&self) -> bool {
        self.conn
            .query_row("SELECT EXISTS(SELECT 1 FROM sessions LIMIT 1)", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|v| v != 0)
            .unwrap_or(false)
    }

    pub fn session_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn prune_before(&self, cutoff: i64) -> Result<usize, String> {
        let deleted = self
            .conn
            .execute("DELETE FROM sessions WHERE ended_at < ?1", [cutoff])
            .map_err(|e| e.to_string())?;
        let _ = self.conn.execute(
            "DELETE FROM apps WHERE id NOT IN (SELECT app_id FROM sessions)",
            [],
        );
        Ok(deleted)
    }

    pub fn delete_all(&self) -> Result<(), String> {
        self.conn
            .execute_batch("BEGIN;\nDELETE FROM sessions;\nDELETE FROM apps;\nCOMMIT;")
            .map_err(|e| e.to_string())
    }

    pub fn size_bytes(&self) -> u64 {
        let main = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        let wal = fs::metadata(format!("{}-wal", self.path.display()))
            .map(|m| m.len())
            .unwrap_or(0);
        main + wal
    }
}

/// One-time repair of rows written before identity resolution was fixed.
///
/// Older builds stored unresolved fallbacks such as `wmclass:gjs` or
/// `wmclass:com.t3tools.t3code` alongside the real application. Re-resolve
/// those rows now, move their sessions to the real desktop entry and delete
/// shell-internal identities so the journal never shows a `gjs` app again.
pub fn migrate_legacy_identities(store: &Store) {
    let version: i64 = match store
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
    {
        Ok(v) => v,
        Err(_) => return,
    };
    if version >= 2 {
        return;
    }
    let index = crate::desktop::DesktopIndex::load();
    let rows: Vec<(i64, String, String, Option<String>)> = {
        let Ok(mut stmt) = store
            .conn
            .prepare("SELECT id, key, name, wm_class FROM apps")
        else {
            return;
        };
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    };
    let now = crate::date::unix_now();
    let _ = store.conn.execute_batch("BEGIN IMMEDIATE");
    for (id, key, name, wm_class) in rows {
        if !(key.starts_with("wmclass:") || key.starts_with("app:")) {
            continue;
        }
        let class = wm_class
            .clone()
            .or_else(|| key.strip_prefix("wmclass:").map(str::to_string));
        if crate::desktop::is_internal_window(class.as_deref(), Some(&name)) {
            // Shell-internal time is not application usage. Cascades sessions.
            let _ = store.conn.execute("DELETE FROM apps WHERE id = ?1", [id]);
            continue;
        }
        let resolved = index.resolve(&key, class.as_deref(), Some(&name));
        if !resolved.matched || resolved.key == key {
            continue;
        }
        if let Ok(target) = store.upsert_app(&resolved, now) {
            if target != id {
                let _ = store.conn.execute(
                    "UPDATE sessions SET app_id = ?1 WHERE app_id = ?2",
                    rusqlite::params![target, id],
                );
                let _ = store.conn.execute("DELETE FROM apps WHERE id = ?1", [id]);
            }
        }
    }
    let _ = store.conn.execute_batch("COMMIT");
    let _ = store.conn.execute_batch("PRAGMA user_version = 2");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Day;
    use crate::stats::{aggregate_day, total_for_day};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daymark-db-{tag}-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn db(&self) -> PathBuf {
            self.0.join("daymark.db")
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn app(key: &str) -> ResolvedApp {
        ResolvedApp {
            key: key.into(),
            name: crate::desktop::prettify(key),
            icon: None,
            desktop_id: None,
            wm_class: None,
            matched: false,
        }
    }

    #[test]
    fn schema_initializes_and_settings_roundtrip() {
        let dir = TempDir::new("init");
        let store = Store::open_at(&dir.db()).unwrap();
        assert_eq!(store.get_setting("nope"), None);
        store.set_setting("theme", "dark").unwrap();
        drop(store);
        let store = Store::open_at(&dir.db()).unwrap();
        assert_eq!(store.get_setting("theme").as_deref(), Some("dark"));
    }

    #[test]
    fn sessions_persist_and_aggregate() {
        let dir = TempDir::new("agg");
        let store = Store::open_at(&dir.db()).unwrap();
        let today = Day::today();
        let id = store
            .upsert_app(&app("org.example.Editor"), today.start_ts())
            .unwrap();
        let other = store
            .upsert_app(&app("org.example.Browser"), today.start_ts())
            .unwrap();
        let s1 = store.open_session(id, today.start_ts() + 3600).unwrap();
        store.close_session(s1, today.start_ts() + 7200).unwrap();
        let s2 = store.open_session(other, today.start_ts() + 7200).unwrap();
        store.close_session(s2, today.start_ts() + 10800).unwrap();
        drop(store);

        let store = Store::open_at(&dir.db()).unwrap();
        let rows = store
            .sessions_between(today.start_ts(), today.end_ts())
            .unwrap();
        assert_eq!(total_for_day(&rows, today), 2 * 3600);
        let agg = aggregate_day(&rows, today);
        assert_eq!(agg.apps.len(), 2);
        assert_eq!(agg.apps[0].1, 3600);
        assert_eq!(store.session_count(), 2);
        let infos = store.app_infos(&[id, other]);
        assert_eq!(infos.len(), 2);
    }

    #[test]
    fn crash_recovery_does_not_credit_downtime() {
        let dir = TempDir::new("recover");
        let today = Day::today();
        let store = Store::open_at(&dir.db()).unwrap();
        let id = store
            .upsert_app(&app("org.example.App"), today.start_ts())
            .unwrap();
        let s = store.open_session(id, today.start_ts() + 600).unwrap();
        store.heartbeat_session(s, today.start_ts() + 900).unwrap();
        // Simulate a crash: the process exits with the row still open.
        drop(store);

        // Several hours later the application starts again.
        let store = Store::open_at(&dir.db()).unwrap();
        let rows = store
            .sessions_between(today.start_ts(), today.end_ts())
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ended, today.start_ts() + 900);
        // A second recovery must not extend it again.
        drop(store);
        let store = Store::open_at(&dir.db()).unwrap();
        let rows = store
            .sessions_between(today.start_ts(), today.end_ts())
            .unwrap();
        assert_eq!(rows[0].ended, today.start_ts() + 900);
    }

    #[test]
    fn at_most_one_open_session_exists() {
        let dir = TempDir::new("open");
        let store = Store::open_at(&dir.db()).unwrap();
        let id = store.upsert_app(&app("a"), 0).unwrap();
        store.open_session(id, 10).unwrap();
        assert!(store.open_session(id, 20).is_err());
    }

    #[test]
    fn prunes_only_old_sessions() {
        let dir = TempDir::new("prune");
        let store = Store::open_at(&dir.db()).unwrap();
        let today = Day::today();
        let id = store.upsert_app(&app("a"), 0).unwrap();
        let old = store
            .open_session(id, today.add_days(-100).start_ts())
            .unwrap();
        store
            .close_session(old, today.add_days(-100).start_ts() + 60)
            .unwrap();
        let recent = store.open_session(id, today.start_ts()).unwrap();
        store.close_session(recent, today.start_ts() + 60).unwrap();
        let removed = store.prune_before(today.add_days(-30).start_ts()).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.session_count(), 1);
    }

    #[test]
    fn a_short_session_merges_into_a_closed_one() {
        let dir = TempDir::new("merge");
        let store = Store::open_at(&dir.db()).unwrap();
        let id = store.upsert_app(&app("a"), 0).unwrap();
        let target = store.open_session(id, 100).unwrap();
        store.close_session(target, 160).unwrap();
        let short = store.open_session(id, 160).unwrap();
        store.merge_short_session(short, target, 164).unwrap();
        assert_eq!(store.session_count(), 1);
        let rows = store.sessions_between(0, 1000).unwrap();
        assert_eq!(rows[0].started, 100);
        assert_eq!(rows[0].ended, 164);
    }

    #[test]
    fn a_session_can_be_reopened_once() {
        let dir = TempDir::new("reopen");
        let store = Store::open_at(&dir.db()).unwrap();
        let id = store.upsert_app(&app("a"), 0).unwrap();
        let s = store.open_session(id, 100).unwrap();
        store.close_session(s, 140).unwrap();
        store.reopen_session(s).unwrap();
        // The reopened row is open again: excluded from day rows, and the
        // unique index still forbids a second open interval.
        assert!(store.sessions_between(0, 1000).unwrap().is_empty());
        assert!(store.open_session(id, 150).is_err());
        store.close_session(s, 200).unwrap();
        let rows = store.sessions_between(0, 1000).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].started, 100);
        assert_eq!(rows[0].ended, 200);
    }

    #[test]
    fn migration_removes_shell_internal_rows() {
        let dir = TempDir::new("migrate");
        let store = Store::open_at(&dir.db()).unwrap();
        let today = Day::today();
        let internal = ResolvedApp {
            key: "wmclass:gjs".into(),
            name: "gjs".into(),
            icon: None,
            desktop_id: None,
            wm_class: Some("gjs".into()),
            matched: false,
        };
        let id = store.upsert_app(&internal, today.start_ts()).unwrap();
        let session = store.open_session(id, today.start_ts() + 10).unwrap();
        store.close_session(session, today.start_ts() + 50).unwrap();
        migrate_legacy_identities(&store);
        assert_eq!(store.session_count(), 0);
        let version: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn recovers_from_a_corrupt_database() {
        let dir = TempDir::new("corrupt");
        fs::write(dir.db(), b"this is definitely not sqlite").unwrap();
        let store = Store::open_at(&dir.db()).unwrap();
        assert!(store.recovered_corrupt_file);
        assert_eq!(store.session_count(), 0);
        let backups: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert!(!backups.is_empty());
    }
}
