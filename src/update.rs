//! Local package update detection. This module reads APT's own metadata and
//! never makes a network request; the package manager remains the update
//! channel. A missing package, a missing APT cache, or a non-Debian host simply
//! reports no update.

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUpdate {
    pub installed: String,
    pub candidate: String,
}

/// Parse the output of `apt-cache policy daymark`.
pub fn parse_apt_policy(text: &str) -> Option<PackageUpdate> {
    let mut installed = None;
    let mut candidate = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Installed:") {
            installed = clean_version(value);
        } else if let Some(value) = line.strip_prefix("Candidate:") {
            candidate = clean_version(value);
        }
    }
    Some(PackageUpdate {
        installed: installed?,
        candidate: candidate?,
    })
}

fn clean_version(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "(none)" {
        None
    } else {
        Some(value.to_string())
    }
}

/// Find the installed version of `package` in a dpkg status file.
pub fn parse_dpkg_status(text: &str, package: &str) -> Option<String> {
    for paragraph in text.split("\n\n") {
        let mut name = None;
        let mut version = None;
        for line in paragraph.lines() {
            if let Some(value) = line.strip_prefix("Package:") {
                name = Some(value.trim());
            } else if let Some(value) = line.strip_prefix("Version:") {
                version = Some(value.trim());
            }
        }
        if name == Some(package) {
            return version.filter(|v| !v.is_empty()).map(str::to_string);
        }
    }
    None
}

/// Numeric dotted versions only. Daymark's own versions are semver-like; a
/// Debian revision suffix is ignored, and an unparseable version is never
/// treated as newer.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    let Some(candidate) = version_parts(candidate) else {
        return false;
    };
    let Some(installed) = version_parts(installed) else {
        return false;
    };
    let length = candidate.len().max(installed.len());
    for index in 0..length {
        let a = candidate.get(index).copied().unwrap_or(0);
        let b = installed.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

fn version_parts(value: &str) -> Option<Vec<u64>> {
    let core = value.split(['-', '+', '~']).next()?;
    core.split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

/// Read the installed version from dpkg's status database.
pub fn installed_version() -> Option<String> {
    let status = std::fs::read_to_string("/var/lib/dpkg/status").ok()?;
    parse_dpkg_status(&status, "daymark")
}

/// Ask APT for its local candidate version. This runs `apt-cache policy`, which
/// reads local lists and never refreshes them. Returns `None` off Debian-family
/// systems or when APT is unavailable.
pub fn apt_candidate() -> Option<String> {
    if !Path::new("/etc/debian_version").is_file() {
        return None;
    }
    let output = std::process::Command::new("apt-cache")
        .args(["policy", "daymark"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    parse_apt_policy(&text).map(|update| update.candidate)
}

/// The update to surface, if APT's local candidate is newer than the installed
/// package. The check is local and read-only.
pub fn available_update() -> Option<PackageUpdate> {
    let installed = installed_version()?;
    let candidate = apt_candidate()?;
    if is_newer(&candidate, &installed) {
        Some(PackageUpdate {
            installed,
            candidate,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apt_policy_parses_installed_and_candidate() {
        let text = "daymark:\n  Installed: 0.1.1\n  Candidate: 0.1.2\n  Version table:\n";
        assert_eq!(
            parse_apt_policy(text),
            Some(PackageUpdate {
                installed: "0.1.1".into(),
                candidate: "0.1.2".into(),
            })
        );
    }

    #[test]
    fn apt_policy_handles_missing_packages() {
        let text = "daymark:\n  Installed: (none)\n  Candidate: (none)\n";
        assert_eq!(parse_apt_policy(text), None);
        assert_eq!(parse_apt_policy("something else\n"), None);
    }

    #[test]
    fn dpkg_status_finds_the_package_paragraph() {
        let text = "Package: other\nVersion: 1.0\n\nPackage: daymark\nStatus: install ok installed\nVersion: 0.1.1\n\nPackage: last\nVersion: 2.0\n";
        assert_eq!(parse_dpkg_status(text, "daymark").as_deref(), Some("0.1.1"));
        assert_eq!(parse_dpkg_status(text, "missing"), None);
    }

    #[test]
    fn numeric_versions_compare_component_wise() {
        assert!(is_newer("0.1.2", "0.1.1"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(!is_newer("0.1.1-1", "0.1.1"));
        assert!(!is_newer("nightly", "0.1.1"));
    }
}
