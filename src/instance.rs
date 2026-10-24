//! Single-instance handling and the tiny local socket used to show an already
//! running window or ask it to quit.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::db;

/// Set by the socket listener, consumed by the UI thread.
pub const COMMAND_NONE: u8 = 0;
pub const COMMAND_SHOW: u8 = 1;
pub const COMMAND_QUIT: u8 = 2;

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

pub fn request_quit() -> bool {
    send("quit")
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
        command: Arc<AtomicU8>,
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
                    match text.trim() {
                        "quit" => {
                            command.store(COMMAND_QUIT, Ordering::SeqCst);
                            if let Some(waker) = &waker {
                                if let Ok(slot) = waker.lock() {
                                    if let Some(thread) = slot.as_ref() {
                                        thread.unpark();
                                    }
                                }
                            }
                            break;
                        }
                        "show" => {
                            command.store(COMMAND_SHOW, Ordering::SeqCst);
                            if let Some(waker) = &waker {
                                if let Ok(slot) = waker.lock() {
                                    if let Some(thread) = slot.as_ref() {
                                        thread.unpark();
                                    }
                                }
                            }
                        }
                        _ => {}
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
        let command = Arc::new(AtomicU8::new(COMMAND_NONE));
        guard
            .listen(Arc::new(AtomicBool::new(false)), command.clone(), None)
            .unwrap();
        assert!(send("show"));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(command.load(Ordering::SeqCst), COMMAND_SHOW);
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }
}
