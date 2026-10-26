//! Desktop integration: autostart, the GNOME extension, theme detection and
//! small XDG helpers. No system-wide files are ever touched.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::db;

pub const EXTENSION_UUID: &str = "daymark@daymark.app";

const EXTENSION_JS: &str = include_str!("../extension/extension.js");
const EXTENSION_METADATA: &str = include_str!("../extension/metadata.json");

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub extension_installed: bool,
    pub extension_enabled: bool,
    pub state_file_present: bool,
    pub state_fresh: bool,
    pub wayland: bool,
}

pub fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

pub fn extension_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/share")
        });
    data.join("gnome-shell/extensions").join(EXTENSION_UUID)
}

/// Where a package puts the companion: one system-wide, read-only copy
/// shared by every user of the machine.
pub fn system_extension_dir() -> PathBuf {
    PathBuf::from("/usr/share/gnome-shell/extensions").join(EXTENSION_UUID)
}

/// True when the packaged system-wide companion is present on disk.
pub fn system_extension_available() -> bool {
    system_extension_dir().join("metadata.json").is_file()
}

pub fn state_path() -> PathBuf {
    db::runtime_dir().join("state.json")
}

/// Which copy of the companion should be enabled for this user. The
/// system-wide copy wins when present so package upgrades apply to everyone
/// without touching the home directory; the user-level copy covers tarball
/// installs where nothing was placed under /usr/share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionSource {
    System,
    User,
    None,
}

/// Pure selection between the system-wide and the user-level copy.
pub fn select_extension_source(system_present: bool, user_present: bool) -> ExtensionSource {
    if system_present {
        ExtensionSource::System
    } else if user_present {
        ExtensionSource::User
    } else {
        ExtensionSource::None
    }
}

/// The companion is usable when either copy exists on disk: the packaged
/// system-wide one or the user-level one written by tarball installs.
pub fn extension_installed() -> bool {
    select_extension_source(system_extension_available(), extension_installed_user())
        != ExtensionSource::None
}

fn extension_installed_user() -> bool {
    extension_dir().join("metadata.json").is_file()
}

fn gsettings_get(key: &str) -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.shell", key])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn parse_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let inner = match (trimmed.find('['), trimmed.rfind(']')) {
        (Some(start), Some(end)) if end > start => &trimmed[start + 1..end],
        _ => "",
    };
    inner
        .split(',')
        .map(|part| part.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn gsettings_set_list(key: &str, values: &[String]) -> Result<(), String> {
    let body = values
        .iter()
        .map(|v| format!("'{}'", v.replace('\'', "")))
        .collect::<Vec<_>>()
        .join(", ");
    let out = Command::new("gsettings")
        .args(["set", "org.gnome.shell", key, &format!("[{body}]")])
        .output()
        .map_err(|e| format!("could not run gsettings: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn extension_enabled() -> bool {
    gsettings_get("enabled-extensions")
        .map(|v| parse_list(&v).iter().any(|s| s == EXTENSION_UUID))
        .unwrap_or(false)
}

pub fn state_age_secs() -> Option<u64> {
    let meta = fs::metadata(state_path()).ok()?;
    let modified = meta.modified().ok()?;
    SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|d| d.as_secs())
}

pub fn capabilities() -> Capabilities {
    let age = state_age_secs();
    Capabilities {
        extension_installed: extension_installed(),
        extension_enabled: extension_enabled(),
        state_file_present: age.is_some(),
        state_fresh: age.is_some_and(|a| a < 45),
        wayland: is_wayland(),
    }
}

fn write_file_if_different(path: &Path, body: &str, mode: u32) -> Result<(), String> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == body {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
            }
            return Ok(());
        }
    }
    let parent = path.parent().ok_or("invalid extension path")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    let mut file =
        fs::File::create(path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    file.write_all(body.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    Ok(())
}

/// Install or update the companion extension and add it to the list GNOME
/// loads at the next login. Returns capabilities after the attempt.
pub fn install_extension() -> Result<Capabilities, String> {
    let dir = extension_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("could not create the extension folder: {e}"))?;
    write_file_if_different(&dir.join("metadata.json"), EXTENSION_METADATA, 0o644)?;
    write_file_if_different(&dir.join("extension.js"), EXTENSION_JS, 0o644)?;

    let mut enabled = gsettings_get("enabled-extensions")
        .map(|v| parse_list(&v))
        .unwrap_or_default();
    if !enabled.iter().any(|s| s == EXTENSION_UUID) {
        enabled.push(EXTENSION_UUID.to_string());
        gsettings_set_list("enabled-extensions", &enabled)
            .map_err(|e| format!("could not enable the extension: {e}"))?;
    }
    if let Some(disabled) = gsettings_get("disabled-extensions") {
        let filtered: Vec<String> = parse_list(&disabled)
            .into_iter()
            .filter(|s| s != EXTENSION_UUID)
            .collect();
        let _ = gsettings_set_list("disabled-extensions", &filtered);
    }

    // Ask a running Shell to load it if it is already known (this succeeds
    // only after the session has seen the extension once). A failed call is
    // expected on a first install and is not an error.
    let _ = Command::new("gnome-extensions")
        .args(["enable", EXTENSION_UUID])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    Ok(capabilities())
}

pub fn remove_extension() -> Result<(), String> {
    let dir = extension_dir();
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("could not remove the extension: {e}"))?;
    }
    if let Some(enabled) = gsettings_get("enabled-extensions") {
        let filtered: Vec<String> = parse_list(&enabled)
            .into_iter()
            .filter(|s| s != EXTENSION_UUID)
            .collect();
        let _ = gsettings_set_list("enabled-extensions", &filtered);
    }
    Ok(())
}

fn quote_exec(path: &str) -> String {
    if !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        return path.to_string();
    }
    let mut out = String::with_capacity(path.len() + 2);
    out.push('"');
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn autostart_body(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Daymark\n\
         Comment=Local screen time tracking\n\
         Exec={} --background\n\
         TryExec={}\n\
         Icon=daymark\n\
         Terminal=false\n\
         StartupNotify=false\n\
         Categories=Utility;\n\
         X-GNOME-Autostart-enabled=true\n",
        quote_exec(&exe.to_string_lossy()),
        exe.to_string_lossy(),
    )
}

pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    let path = db::autostart_path();
    if !enabled {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("could not remove the autostart entry: {e}"))?;
        }
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| format!("could not locate Daymark: {e}"))?;
    let parent = path.parent().ok_or("invalid autostart path")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("could not create the autostart folder: {e}"))?;
    let body = autostart_body(&exe);
    if fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
        fs::write(&path, body).map_err(|e| format!("could not write the autostart entry: {e}"))?;
    }
    Ok(())
}

pub fn autostart_installed() -> bool {
    db::autostart_path().is_file()
}

/// Read GNOME's colour preference once. A missing or unusual desktop falls
/// back to the light theme rather than an error.
pub fn system_prefers_dark() -> bool {
    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .stderr(Stdio::null())
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if text.contains("dark") {
                return true;
            }
            if text.contains("light") || text.contains("default") {
                return false;
            }
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home {
        for rel in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
            if let Ok(text) = fs::read_to_string(home.join(".config").join(rel)) {
                for line in text.lines() {
                    let line = line.trim().to_lowercase().replace(' ', "");
                    if line.starts_with("gtk-application-prefer-dark-theme=") {
                        if line.ends_with("=1") || line.ends_with("=true") {
                            return true;
                        }
                        if line.ends_with("=0") || line.ends_with("=false") {
                            return false;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Watch GNOME's colour scheme with a single long-lived `gsettings monitor`
/// child. Each line flips `signal`; the UI thread re-reads the value on its own
/// timer, so no Slint type ever crosses a thread boundary.
///
/// Returns `false` when the monitor could not be started. The caller can then
/// fall back to sampling the scheme periodically.
pub fn watch_color_scheme(signal: std::sync::Arc<std::sync::atomic::AtomicBool>) -> bool {
    let mut child = match Command::new("gsettings")
        .args(["monitor", "org.gnome.desktop.interface", "color-scheme"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    std::thread::Builder::new()
        .name("daymark-theme".into())
        .spawn(move || {
            use std::io::BufRead;
            if let Some(stdout) = child.stdout.take() {
                for line in std::io::BufReader::new(stdout).lines() {
                    if line.is_err() {
                        break;
                    }
                    signal.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            let _ = child.wait();
        })
        .is_ok()
}

pub fn spawn_detached(program: &str, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not launch {program}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn open_data_folder() -> Result<(), String> {
    let dir = db::data_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    spawn_detached("xdg-open", &[dir.to_string_lossy().as_ref()])
}

pub fn open_file_manager(path: &Path) -> Result<(), String> {
    spawn_detached("xdg-open", &[path.to_string_lossy().as_ref()])
}

pub fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gvariant_lists() {
        assert_eq!(
            parse_list("['a@b.com', 'c@d.com']"),
            vec!["a@b.com".to_string(), "c@d.com".to_string()]
        );
        assert_eq!(parse_list("@as []"), Vec::<String>::new());
    }

    #[test]
    fn the_system_wide_copy_wins_over_the_user_copy() {
        assert_eq!(select_extension_source(true, true), ExtensionSource::System);
        assert_eq!(
            select_extension_source(true, false),
            ExtensionSource::System
        );
        assert_eq!(select_extension_source(false, true), ExtensionSource::User);
        assert_eq!(select_extension_source(false, false), ExtensionSource::None);
    }

    #[test]
    fn the_system_extension_lives_outside_the_home_directory() {
        assert_eq!(
            system_extension_dir(),
            PathBuf::from("/usr/share/gnome-shell/extensions").join(EXTENSION_UUID)
        );
        assert_eq!(system_extension_dir().file_name().unwrap(), EXTENSION_UUID);
    }

    #[test]
    fn quotes_exec_paths_per_desktop_entry_rules() {
        assert_eq!(quote_exec("/usr/bin/daymark"), "/usr/bin/daymark");
        assert_eq!(quote_exec("/home/a b/daymark"), "\"/home/a b/daymark\"");
        assert_eq!(quote_exec("/tmp/a$b"), "\"/tmp/a\\$b\"");
    }

    #[test]
    fn autostart_file_contains_the_background_flag() {
        let body = autostart_body(Path::new("/usr/bin/daymark"));
        assert!(body.contains("Exec=/usr/bin/daymark --background"));
        assert!(body.contains("X-GNOME-Autostart-enabled=true"));
        assert!(body.starts_with("[Desktop Entry]"));
    }
}
