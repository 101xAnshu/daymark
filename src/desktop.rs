//! Resolution of window identities to human names, desktop files and icons.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct DesktopEntry {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub exec: Option<String>,
    pub wm_class: Option<String>,
    pub file: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ResolvedApp {
    pub key: String,
    pub name: String,
    pub icon: Option<String>,
    pub desktop_id: Option<String>,
    pub wm_class: Option<String>,
    pub matched: bool,
}

#[derive(Default)]
pub struct DesktopIndex {
    entries: Vec<DesktopEntry>,
    by_id: HashMap<String, usize>,
    by_class: HashMap<String, usize>,
    by_exec: HashMap<String, usize>,
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(home));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    if let Some(list) = std::env::var_os("XDG_DATA_DIRS") {
        for part in std::env::split_paths(&list) {
            dirs.push(part);
        }
    } else {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    }
    // Common locations that are not always in XDG_DATA_DIRS.
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop"));
    dirs
}

fn parse_desktop_file(path: &Path, id: String) -> Option<DesktopEntry> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 512 * 1024 {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name = None;
    let mut icon = None;
    let mut exec = None;
    let mut wm_class = None;
    let mut hidden = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Name" => name = Some(value.to_string()),
            "Icon" => icon = Some(value.to_string()),
            "Exec" => exec = Some(value.to_string()),
            "StartupWMClass" => wm_class = Some(value.to_string()),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    // `NoDisplay` only hides an entry from launchers. We never show a launcher,
    // and several real applications (T3 Code, for one) publish their only
    // identity and icon through a NoDisplay entry, so keep it in the index.
    if hidden {
        return None;
    }
    Some(DesktopEntry {
        id,
        name: name?,
        icon: icon.filter(|i| !i.is_empty()),
        exec,
        wm_class: wm_class.filter(|w| !w.is_empty()),
        file: path.to_path_buf(),
    })
}

/// Split an `Exec` line into arguments the way the desktop-entry spec does:
/// double quotes group words, backslash escapes the next character.
fn exec_tokens(exec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut chars = exec.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                has_token = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            if let Some(&next) = chars.peek() {
                                chars.next();
                                current.push(next);
                            }
                        }
                        c => current.push(c),
                    }
                }
            }
            '\'' => {
                has_token = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    current.push(c);
                }
            }
            c if c.is_whitespace() => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    has_token = true;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        tokens.push(current);
    }
    tokens
}

fn exec_basename(exec: &str) -> Option<String> {
    for token in exec_tokens(exec) {
        let lower = token.to_ascii_lowercase();
        // `env VAR=value command` and `env -u VAR command` wrappers.
        if lower == "env" {
            continue;
        }
        if lower.starts_with('-') {
            continue;
        }
        if token.contains('=') && !token.contains('/') {
            continue; // VAR=value
        }
        return Path::new(&token)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase());
    }
    None
}

/// Identities that belong to the desktop shell itself, never to a user app.
/// They can still surface as focused windows through Mutter, especially on
/// GNOME (gjs hosts the shell's notifications, screen saver and desktop icons).
pub fn is_internal_window(wm_class: Option<&str>, name: Option<&str>) -> bool {
    let class = wm_class
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '_'], "-");
    if matches!(
        class.as_str(),
        "gjs" | "gnome-shell" | "gnome-shell-extension" | "ding" | "plasmashell" | "kwin_wayland"
    ) {
        return true;
    }
    let name = name.unwrap_or("").trim().to_ascii_lowercase();
    class.is_empty() && matches!(name.as_str(), "gjs" | "gnome-shell")
}

impl DesktopIndex {
    pub fn load() -> Self {
        let mut index = DesktopIndex::default();
        let mut seen: HashMap<String, ()> = HashMap::new();
        for base in data_dirs() {
            let dir = base.join("applications");
            let Ok(read) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in read.flatten() {
                let path = entry.path();
                let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !file_name.ends_with(".desktop") {
                    continue;
                }
                if seen.contains_key(file_name) {
                    continue; // earlier data dirs take precedence
                }
                if let Some(entry) = parse_desktop_file(&path, file_name.to_string()) {
                    seen.insert(file_name.to_string(), ());
                    index.entries.push(entry);
                }
            }
        }
        index.reindex();
        index
    }

    fn reindex(&mut self) {
        self.by_id.clear();
        self.by_class.clear();
        self.by_exec.clear();
        for (i, entry) in self.entries.iter().enumerate() {
            let base = entry
                .id
                .strip_suffix(".desktop")
                .unwrap_or(&entry.id)
                .to_lowercase();
            self.by_id.entry(base).or_insert(i);
            self.by_id.entry(entry.id.to_lowercase()).or_insert(i);
            if let Some(class) = &entry.wm_class {
                self.by_class.entry(class.to_lowercase()).or_insert(i);
            }
            if let Some(exec) = entry.exec.as_deref().and_then(exec_basename) {
                self.by_exec.entry(exec).or_insert(i);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn find_by_id(&self, app_id: &str) -> Option<&DesktopEntry> {
        let base = app_id
            .strip_suffix(".desktop")
            .unwrap_or(app_id)
            .to_lowercase();
        self.by_id
            .get(&base)
            .or_else(|| self.by_id.get(&app_id.to_lowercase()))
            .map(|i| &self.entries[*i])
    }

    fn find_by_class(&self, wm_class: &str) -> Option<&DesktopEntry> {
        let class = wm_class.to_lowercase();
        if let Some(i) = self.by_class.get(&class).or_else(|| self.by_id.get(&class)) {
            return self.entries.get(*i);
        }
        self.entries
            .iter()
            .find(|e| {
                e.id.strip_suffix(".desktop")
                    .is_some_and(|id| id.eq_ignore_ascii_case(wm_class))
                    || e.wm_class
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(wm_class))
            })
            .or_else(|| self.find_by_exec(&class))
    }

    fn find_by_exec(&self, class: &str) -> Option<&DesktopEntry> {
        self.by_exec.get(class).map(|i| &self.entries[*i])
    }

    pub fn resolve(
        &self,
        app_id: &str,
        wm_class: Option<&str>,
        shell_name: Option<&str>,
    ) -> ResolvedApp {
        let app_id = app_id.trim();
        let is_window_backed = app_id.starts_with("wmclass:") || app_id.starts_with("window:");
        let id_part = app_id
            .strip_prefix("wmclass:")
            .or_else(|| app_id.strip_prefix("window:"))
            .unwrap_or(app_id);

        let entry = if !is_window_backed {
            self.find_by_id(app_id)
        } else {
            None
        }
        .or_else(|| wm_class.and_then(|c| self.find_by_class(c)))
        .or_else(|| {
            if !id_part.is_empty() {
                self.find_by_class(id_part)
            } else {
                None
            }
        })
        .or_else(|| wm_class.and_then(|c| self.find_by_exec(&c.to_lowercase())));

        if let Some(entry) = entry {
            ResolvedApp {
                key: entry.id.clone(),
                name: entry.name.clone(),
                icon: entry.icon.clone().or_else(|| {
                    Some(
                        entry
                            .id
                            .strip_suffix(".desktop")
                            .unwrap_or(&entry.id)
                            .to_string(),
                    )
                }),
                desktop_id: Some(entry.id.clone()),
                wm_class: wm_class.map(str::to_string),
                matched: true,
            }
        } else {
            let raw = wm_class
                .filter(|c| !c.is_empty() && !contains_reverse_dns_noise(c))
                .or_else(|| {
                    let p = id_part
                        .strip_prefix("org.")
                        .or_else(|| id_part.strip_prefix("com."))
                        .or_else(|| id_part.strip_prefix("io."))
                        .or_else(|| id_part.strip_prefix("net."))
                        .unwrap_or(id_part);
                    Some(p)
                })
                .unwrap_or("Application");
            let name = shell_name
                .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("unknown"))
                .map(str::to_string)
                .unwrap_or_else(|| prettify(raw));
            let key = if !wm_class.unwrap_or("").is_empty() {
                format!("wmclass:{}", wm_class.unwrap_or("").to_lowercase())
            } else {
                format!("app:{}", id_part.to_lowercase())
            };
            ResolvedApp {
                key,
                name,
                icon: None,
                desktop_id: None,
                wm_class: wm_class.map(str::to_string),
                matched: false,
            }
        }
    }
}

fn contains_reverse_dns_noise(s: &str) -> bool {
    // Purely reverse-DNS identifiers make poor display names; prefer the
    // desktop id's final component in that case.
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 3 && parts[0].len() == 2 && parts[1].len() >= 3
}

pub fn prettify(raw: &str) -> String {
    let mut base = raw;
    if base.contains('.') {
        if let Some(last) = base.rsplit('.').next() {
            base = last;
        }
    }
    base = base
        .strip_suffix(".desktop")
        .unwrap_or(base)
        .trim_matches(|c: char| c == '"' || c == '\'');
    let spaced = base.replace(['-', '_'], " ");
    let mut out = String::with_capacity(spaced.len());
    let mut capitalize = true;
    for c in spaced.chars() {
        if c.is_whitespace() {
            capitalize = true;
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else if capitalize {
            out.extend(c.to_uppercase());
            capitalize = false;
        } else {
            out.push(c);
        }
    }
    if out.trim().is_empty() {
        "Application".into()
    } else {
        out.trim().to_string()
    }
}

fn home_icons() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(home).join("icons"));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/icons"));
    }
    if let Some(list) = std::env::var_os("XDG_DATA_DIRS") {
        for part in std::env::split_paths(&list) {
            roots.push(part.join("icons"));
        }
    } else {
        roots.push(PathBuf::from("/usr/share/icons"));
        roots.push(PathBuf::from("/usr/local/share/icons"));
    }
    roots.push(PathBuf::from("/usr/share/pixmaps"));
    roots
}

/// Icon themes to try, after the user's active theme. `hicolor` is the
/// specification-mandated fallback and covers most installed applications.
const FALLBACK_THEMES: &[&str] = &["hicolor", "Adwaita", "Yaru", "Papirus", "breeze"];
const SIZES: &[&str] = &[
    "64x64", "48x48", "96x96", "128x128", "32x32", "256x256", "512x512",
];

fn active_icon_theme() -> String {
    static THEME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    THEME
        .get_or_init(|| {
            let out = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "icon-theme"])
                .stderr(std::process::Stdio::null())
                .output();
            out.ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .trim_matches('\'')
                        .trim_matches('"')
                        .to_string()
                })
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Yaru".into())
        })
        .clone()
}

/// Locate an icon for an icon name. PNG is preferred for its predictable cost,
/// but SVG is accepted (Slint 1.8's `std` build already includes the renderer),
/// so applications that only ship a scalable icon still get their real mark.
/// Symbolic monochrome variants are skipped because they lose their meaning
/// when shown as full-colour tiles.
pub fn find_icon(icon: &str) -> Option<PathBuf> {
    let trimmed = icon.trim();
    if trimmed.is_empty() {
        return None;
    }
    let as_path = Path::new(trimmed);
    if as_path.is_absolute() && as_path.is_file() {
        let ext = as_path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("png" | "svg" | "svgz")) {
            return Some(as_path.to_path_buf());
        }
    }
    let stem = trimmed
        .strip_suffix(".png")
        .or_else(|| trimmed.strip_suffix(".svg"))
        .or_else(|| trimmed.strip_suffix(".svgz"))
        .or_else(|| trimmed.strip_suffix(".xpm"))
        .unwrap_or(trimmed);
    if stem.ends_with("-symbolic") {
        return None;
    }
    let active = active_icon_theme();
    let mut themes = vec![active];
    for theme in FALLBACK_THEMES {
        if !themes.iter().any(|t| t.eq_ignore_ascii_case(theme)) {
            themes.push((*theme).to_string());
        }
    }
    let roots = home_icons();
    for theme in &themes {
        for root in &roots {
            let theme_dir = root.join(theme);
            if !theme_dir.is_dir() {
                continue;
            }
            for size in SIZES {
                let apps = theme_dir.join(size).join("apps");
                for ext in ["png", "svg"] {
                    let candidate = apps.join(format!("{stem}.{ext}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
            let scalable = theme_dir.join("scalable").join("apps");
            for ext in ["svg", "png"] {
                let candidate = scalable.join(format!("{stem}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    // Last chance: a few apps only ship one unscaled icon in pixmaps.
    for root in &roots {
        if root.ends_with("pixmaps") {
            for ext in ["png", "svg", "xpm"] {
                let candidate = root.join(format!("{stem}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_desktop_entry() {
        let dir = std::env::temp_dir().join(format!("daymark-desktop-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("com.example.Test.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=Example App\nIcon=example\nExec=/usr/bin/example --flag %U\nStartupWMClass=ExampleClass\n",
        )
        .unwrap();
        let entry = parse_desktop_file(&path, "com.example.Test.desktop".into()).unwrap();
        assert_eq!(entry.name, "Example App");
        assert_eq!(entry.icon.as_deref(), Some("example"));
        assert_eq!(entry.wm_class.as_deref(), Some("ExampleClass"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignores_hidden_entries_but_keeps_nodisplay_ones() {
        let dir = std::env::temp_dir().join(format!(
            "daymark-desktop-test-hidden-{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&dir);
        let hidden = dir.join("hidden.desktop");
        fs::write(&hidden, "[Desktop Entry]\nName=Hidden\nHidden=true\n").unwrap();
        assert!(parse_desktop_file(&hidden, "hidden.desktop".into()).is_none());

        // A NoDisplay entry is the only identity some apps publish.
        let quiet = dir.join("com.example.Quiet.desktop");
        fs::write(
            &quiet,
            "[Desktop Entry]\nName=Quiet\nNoDisplay=true\nIcon=/tmp/quiet.png\n",
        )
        .unwrap();
        let entry = parse_desktop_file(&quiet, "com.example.Quiet.desktop".into()).unwrap();
        assert_eq!(entry.name, "Quiet");
        assert_eq!(entry.icon.as_deref(), Some("/tmp/quiet.png"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_quoted_exec_lines() {
        assert_eq!(
            exec_basename("\"/opt/T3 Code (Nightly)/t3code\" %U"),
            Some("t3code".into())
        );
        assert_eq!(
            exec_basename("env FOO=bar /usr/bin/example --flag"),
            Some("example".into())
        );
        assert_eq!(exec_basename("example"), Some("example".into()));
    }

    #[test]
    fn recognises_shell_internal_windows() {
        assert!(is_internal_window(Some("gjs"), Some("gjs")));
        assert!(is_internal_window(Some("GNOME Shell"), None));
        assert!(is_internal_window(Some("ding"), None));
        assert!(!is_internal_window(Some("com.t3tools.T3Code"), None));
        assert!(!is_internal_window(Some("brave-browser"), Some("Brave")));
    }

    #[test]
    fn prettifies_executableish_names() {
        assert_eq!(prettify("brave-browser"), "Brave Browser");
        assert_eq!(prettify("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(prettify("simple_scan"), "Simple Scan");
        assert_eq!(prettify(""), "Application");
    }

    #[test]
    fn resolves_fallback_identity() {
        let index = DesktopIndex::default();
        let app = index.resolve("brave-browser", Some("brave-browser"), None);
        assert_eq!(app.name, "Brave Browser");
        assert_eq!(app.key, "wmclass:brave-browser");
        assert!(!app.matched);
    }
}
