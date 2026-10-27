//! Single-instance handling and the tiny local socket used to show an already
//! running window or ask it to quit.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::db;

/// Set by the socket listener, consumed by the UI thread. The low byte is the
/// command; the upper bits carry a pause length when the command is
/// [`COMMAND_PAUSE`]. One atomic keeps the pair consistent for the reader.
pub const COMMAND_NONE: u64 = 0;
pub const COMMAND_SHOW: u64 = 1;
pub const COMMAND_QUIT: u64 = 2;
pub const COMMAND_PAUSE: u64 = 3;
pub const COMMAND_RESUME: u64 = 4;

const COMMAND_MASK: u64 = 0xff;
const PAUSE_SHIFT: u32 = 8;

/// A pause longer than this is clamped. The menu only uses 15 minutes and one
/// hour, but the socket is a local interface and should stay bounded.
pub const MAX_PAUSE_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Encode a pause length (0 means "until resumed") into the command word.
pub fn pause_command(seconds: i64) -> u64 {
    let seconds = seconds.clamp(0, MAX_PAUSE_SECONDS) as u64;
    COMMAND_PAUSE | (seconds << PAUSE_SHIFT)
}

/// The command byte, with any pause payload removed.
pub fn command_kind(command: u64) -> u64 {
    command & COMMAND_MASK
}

/// The pause length carried by a [`COMMAND_PAUSE`] word.
pub fn pause_seconds(command: u64) -> i64 {
    (command >> PAUSE_SHIFT) as i64
}

/// Parse one line from the local socket. Pure so the command mapping can be
/// tested without a listener or a live application.
fn parse_command(text: &str) -> Option<u64> {
    match text {
        "quit" => Some(COMMAND_QUIT),
        "show" => Some(COMMAND_SHOW),
        "resume" => Some(COMMAND_RESUME),
        "pause" => Some(pause_command(0)),
        other => other
            .strip_prefix("pause ")
            .and_then(|seconds| seconds.trim().parse::<i64>().ok())
            .map(pause_command),
    }
}

pub struct InstanceGuard {
    _lock: std::fs::File,
    listener: UnixListener,
    path: PathBuf,
}

fn socket_path() -> PathBuf {
    db::runtime_dir().join("daymark.sock")
}

fn lock_path() -> PathBuf {
    db::runtime_dir().join("daymark.lock")
}

fn send(command: &str) -> bool {
    let path = socket_path();
    for _ in 0..12 {
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(format!("{command}\n").as_bytes());
            let _ = stream.flush();
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

pub fn request_show() -> bool {
    send("show")
}

pub fn request_quit() -> bool {
    send("quit")
}

/// Pause for `seconds`, or until the user resumes when it is zero.
pub fn request_pause(seconds: i64) -> bool {
    send(&format!("pause {}", seconds.clamp(0, MAX_PAUSE_SECONDS)))
}

pub fn request_resume() -> bool {
    send("resume")
}

impl InstanceGuard {
    /// Returns `None` when another instance owns the lock (after asking it to
    /// show its window).
    pub fn acquire() -> Result<Option<Self>, String> {
        let dir = db::runtime_dir();
        fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create the runtime folder: {e}"))?;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path())
            .map_err(|e| format!("could not open the instance lock: {e}"))?;
        // SAFETY: flock only operates on the valid descriptor owned by `lock`.
        let rc = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                let _ = send("show");
                return Ok(None);
            }
            return Err(format!("could not lock the instance file: {err}"));
        }

        let path = socket_path();
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)
            .map_err(|e| format!("could not create the local socket: {e}"))?;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        listener
            .set_nonblocking(false)
            .map_err(|e| format!("could not configure the local socket: {e}"))?;
        Ok(Some(Self {
            _lock: lock,
            listener,
            path,
        }))
    }

    /// Start the IPC listener. It only sets an atomic command; the UI thread
    /// consumes it, so no Slint types ever cross the thread boundary. When a
    /// `waker` is provided (the invisible background process), the listener
    /// unparks that thread so it reacts immediately instead of polling.
    pub fn listen(
        &self,
        shutting_down: Arc<AtomicBool>,
        command: Arc<AtomicU64>,
        waker: Option<Arc<Mutex<Option<std::thread::Thread>>>>,
    ) -> Result<(), String> {
        let listener = self
            .listener
            .try_clone()
            .map_err(|e| format!("could not clone the local socket: {e}"))?;
        std::thread::Builder::new()
            .name("daymark-ipc".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if shutting_down.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(mut stream) = stream else {
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut buf = [0u8; 32];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let text = String::from_utf8_lossy(&buf[..n]);
                    let Some(value) = parse_command(text.trim()) else {
                        continue;
                    };
                    command.store(value, Ordering::SeqCst);
                    if let Some(waker) = &waker {
                        if let Ok(slot) = waker.lock() {
                            if let Some(thread) = slot.as_ref() {
                                thread.unpark();
                            }
                        }
                    }
                    if command_kind(value) == COMMAND_QUIT {
                        break;
                    }
                }
            })
            .map_err(|e| format!("could not start the instance listener: {e}"))?;
        Ok(())
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_roundtrip() {
        let _ = crate::db::runtime_dir();
        // Use a unique runtime dir so the developer's own Daymark instance is
        // never disturbed by the test.
        let dir = std::env::temp_dir().join(format!("daymark-ipc-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        // SAFETY: setting a process-wide variable is only done in this test
        // binary, and no other test relies on XDG_RUNTIME_DIR.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &dir) };
        let guard = InstanceGuard::acquire().unwrap().unwrap();
        let command = Arc::new(AtomicU64::new(COMMAND_NONE));
        guard
            .listen(Arc::new(AtomicBool::new(false)), command.clone(), None)
            .unwrap();
        assert!(send("show"));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(command.load(Ordering::SeqCst), COMMAND_SHOW);
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pause_commands_carry_their_length() {
        let fifteen = pause_command(900);
        assert_eq!(command_kind(fifteen), COMMAND_PAUSE);
        assert_eq!(pause_seconds(fifteen), 900);
        assert_eq!(command_kind(COMMAND_SHOW), COMMAND_SHOW);
        assert_eq!(command_kind(COMMAND_QUIT), COMMAND_QUIT);
        assert_eq!(command_kind(COMMAND_RESUME), COMMAND_RESUME);
    }

    #[test]
    fn socket_lines_map_to_commands() {
        assert_eq!(parse_command("show"), Some(COMMAND_SHOW));
        assert_eq!(parse_command("quit"), Some(COMMAND_QUIT));
        assert_eq!(parse_command("resume"), Some(COMMAND_RESUME));
        assert_eq!(parse_command("pause"), Some(pause_command(0)));
        assert_eq!(parse_command("pause 900"), Some(pause_command(900)));
        assert_eq!(parse_command("pause 3600"), Some(pause_command(3600)));
        assert_eq!(parse_command("pause 0"), Some(pause_command(0)));
        assert_eq!(
            parse_command("pause 999999999"),
            Some(pause_command(MAX_PAUSE_SECONDS))
        );
        assert_eq!(parse_command("pause -5"), Some(pause_command(0)));
        assert_eq!(parse_command("unknown"), None);
    }
}
