//! A tiny append-only log for lifecycle events, timings and errors.
//!
//! Files live under `$XDG_STATE_HOME/daymark/logs` (or the XDG fallback) and
//! rotate once per local day. Seven days are kept. Callers log aggregate
//! counts, event timings and error text only: never window titles, URLs,
//! search terms or application content.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::date::{self, Day};

const KEEP_DAYS: u64 = 7;

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn home() -> PathBuf {
    env_path("HOME").unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Root for state that should survive reboots but is not user data.
pub fn state_dir() -> PathBuf {
    env_path("XDG_STATE_HOME")
        .unwrap_or_else(|| home().join(".local/state"))
        .join("daymark")
}

pub fn log_dir() -> PathBuf {
    state_dir().join("logs")
}

fn ensure_private_dir(path: &Path) {
    let _ = fs::create_dir_all(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
}

/// Local wall-clock stamp in `YYYY-MM-DD HH:MM:SS` form. Kept here so doctor
/// output and log lines agree without pulling in a date-time dependency.
pub fn format_timestamp(ts: i64) -> String {
    // SAFETY: localtime_r writes only to the provided struct and reads a valid
    // time value owned by this function.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let value = ts as libc::time_t;
    unsafe {
        libc::localtime_r(&value, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

fn log_file(dir: &Path, ts: i64) -> PathBuf {
    dir.join(format!("daymark-{}.log", Day::from_timestamp(ts).iso()))
}

fn prune_old(dir: &Path, now: i64) {
    let Some(cutoff) = now
        .checked_sub((KEEP_DAYS * 86_400) as i64)
        .map(|value| UNIX_EPOCH + std::time::Duration::from_secs(value.max(0) as u64))
    else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("daymark-") || !name.ends_with(".log") {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .map(|modified| modified < cutoff)
            .unwrap_or(false);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn write_line(dir: &Path, ts: i64, level: &str, event: &str, message: &str) {
    ensure_private_dir(dir);
    prune_old(dir, ts);
    let line = format!(
        "{} level={} event={} message={}\n",
        format_timestamp(ts),
        one_line(level),
        one_line(event),
        one_line(message)
    );
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(log_file(dir, ts)) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Prepare the directory and drop files older than seven days.
pub fn init() {
    let dir = log_dir();
    ensure_private_dir(&dir);
    prune_old(&dir, date::unix_now());
}

pub fn info(event: &str, message: &str) {
    write_line(&log_dir(), date::unix_now(), "INFO", event, message);
}

pub fn error(event: &str, message: &str) {
    write_line(&log_dir(), date::unix_now(), "ERROR", event, message);
}

/// The most recent error line, as `timestamp event=… message=…`.
pub fn last_error(dir: &Path) -> Option<String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .map(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("daymark-") && name.ends_with(".log")
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    let mut last = None;
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            let Some(index) = line.find(" level=ERROR ") else {
                continue;
            };
            let stamp = line.get(..index).unwrap_or("");
            let rest = line.get(index + 1..).unwrap_or("");
            if let Some(rest) = rest.strip_prefix("level=ERROR ") {
                last = Some(format!("{stamp} {rest}"));
            }
        }
    }
    last
}

/// A system time a number of seconds in the past, used only by tests that
/// need a stable file timestamp.
#[cfg(test)]
fn system_time_ago(seconds: u64) -> std::time::SystemTime {
    std::time::SystemTime::now() - std::time::Duration::from_secs(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daymark-log-{tag}-{}-{}",
                std::process::id(),
                date::unix_now()
            ));
            let _ = fs::create_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn writes_one_line_errors_and_finds_the_latest() {
        let dir = TempDir::new("errors");
        write_line(&dir.0, 1_700_000_000, "INFO", "started", "mode=window");
        write_line(&dir.0, 1_700_000_010, "ERROR", "database", "first\nproblem");
        write_line(&dir.0, 1_700_000_020, "ERROR", "save", "second problem");
        let last = last_error(&dir.0).unwrap();
        assert!(last.ends_with("event=save message=second problem"));
        assert!(!last.contains('\n'));
    }

    #[test]
    fn missing_log_directory_has_no_last_error() {
        assert_eq!(last_error(Path::new("/nonexistent/daymark/logs")), None);
    }

    #[test]
    fn prunes_files_older_than_seven_days() {
        let dir = TempDir::new("prune");
        let now = date::unix_now();
        let old = dir.0.join("daymark-2000-01-01.log");
        let fresh = dir
            .0
            .join(format!("daymark-{}.log", Day::from_timestamp(now).iso()));
        fs::write(&old, "old\n").unwrap();
        fs::write(&fresh, "fresh\n").unwrap();
        let times = fs::FileTimes::new().set_modified(system_time_ago(8 * 86_400));
        let file = fs::File::options().write(true).open(&old).unwrap();
        file.set_times(times).unwrap();
        prune_old(&dir.0, now);
        assert!(!old.exists());
        assert!(fresh.exists());
    }
}
