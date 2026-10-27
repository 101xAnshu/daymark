//! Domain-only site names. This module never keeps a path, query, title or
//! full URL; it extracts the registrable host from a web URL and can read the
//! browser's local history read-only to build a domain rollup. It is only
//! called when the user opts in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};

use crate::date::Day;
use crate::db::Store;

/// A single history visit. The URL stays in memory only long enough to extract
/// its domain; it is never stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryVisit {
    pub url: String,
    pub visited_at: i64,
}

/// Longest gap credited to one page. A browser left open overnight must not
/// turn into a night of site time.
const MAX_DOMAIN_GAP: i64 = 30 * 60;
/// Chromium timestamps count microseconds from 1601-01-01.
const CHROMIUM_EPOCH_OFFSET: i64 = 11_644_473_600;

/// Extract the host from an http(s) URL. Returns `None` for non-web schemes,
/// malformed input, localhost and IP literals.
pub fn domain_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    let host = authority.rsplit('@').next()?.trim();
    let host = host.split(':').next()?.trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if host == "localhost"
        || host.parse::<std::net::IpAddr>().is_ok()
        || !host.contains('.')
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
    {
        return None;
    }
    Some(host.to_string())
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let _ = connection.execute_batch("PRAGMA query_only = ON;");
    Ok(connection)
}

/// Read Firefox history visits in `[start, end)` (Unix seconds).
pub fn read_firefox_history(
    path: &Path,
    start: i64,
    end: i64,
) -> Result<Vec<HistoryVisit>, String> {
    let connection = open_read_only(path)?;
    let mut statement = connection
        .prepare(
            "SELECT p.url, v.visit_date FROM moz_historyvisits v\n\
             JOIN moz_places p ON p.id = v.place_id\n\
             WHERE p.hidden = 0 AND v.visit_date >= ?1 AND v.visit_date < ?2\n\
             ORDER BY v.visit_date",
        )
        .map_err(|e| format!("could not read Firefox history: {e}"))?;
    let rows = statement
        .query_map(
            params![
                start.saturating_mul(1_000_000),
                end.saturating_mul(1_000_000)
            ],
            |row| {
                let url: String = row.get(0)?;
                let micros: i64 = row.get(1)?;
                Ok(HistoryVisit {
                    url,
                    visited_at: micros / 1_000_000,
                })
            },
        )
        .map_err(|e| format!("could not read Firefox history: {e}"))?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Read Chromium-family history visits in `[start, end)` (Unix seconds).
pub fn read_chromium_history(
    path: &Path,
    start: i64,
    end: i64,
) -> Result<Vec<HistoryVisit>, String> {
    let connection = open_read_only(path)?;
    let mut statement = connection
        .prepare(
            "SELECT u.url, v.visit_time FROM visits v\n\
             JOIN urls u ON u.id = v.url\n\
             WHERE u.hidden = 0 AND v.visit_time >= ?1 AND v.visit_time < ?2\n\
             ORDER BY v.visit_time",
        )
        .map_err(|e| format!("could not read Chromium history: {e}"))?;
    let rows = statement
        .query_map(
            params![
                (start.saturating_add(CHROMIUM_EPOCH_OFFSET)).saturating_mul(1_000_000),
                (end.saturating_add(CHROMIUM_EPOCH_OFFSET)).saturating_mul(1_000_000)
            ],
            |row| {
                let url: String = row.get(0)?;
                let micros: i64 = row.get(1)?;
                Ok(HistoryVisit {
                    url,
                    visited_at: micros / 1_000_000 - CHROMIUM_EPOCH_OFFSET,
                })
            },
        )
        .map_err(|e| format!("could not read Chromium history: {e}"))?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Attribute time between consecutive visits to the earlier page's domain.
/// The last visit is credited up to `end`, capped by [`MAX_DOMAIN_GAP`].
/// This is an approximation, not an exact measure of reading time.
pub fn domain_seconds(visits: &[HistoryVisit], start: i64, end: i64) -> Vec<(String, i64)> {
    let mut sorted: Vec<&HistoryVisit> = visits
        .iter()
        .filter(|visit| visit.visited_at >= start && visit.visited_at < end)
        .collect();
    sorted.sort_by_key(|visit| visit.visited_at);
    let mut totals: HashMap<String, i64> = HashMap::new();
    for (index, visit) in sorted.iter().enumerate() {
        let Some(domain) = domain_from_url(&visit.url) else {
            continue;
        };
        let next = sorted
            .get(index + 1)
            .map(|next| next.visited_at)
            .unwrap_or(end);
        let gap = (next - visit.visited_at).clamp(0, MAX_DOMAIN_GAP);
        *totals.entry(domain).or_insert(0) += gap;
    }
    let mut out: Vec<(String, i64)> = totals.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Firefox profiles with a history database.
pub fn firefox_history_paths(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home.join(".mozilla/firefox")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path().join("places.sqlite"))
        .filter(|path| path.is_file())
        .collect()
}

/// Chromium-family profiles with a history database.
pub fn chromium_history_paths(home: &Path) -> Vec<PathBuf> {
    let roots = [
        ".config/google-chrome",
        ".config/chromium",
        ".config/BraveSoftware/Brave-Browser",
        ".config/microsoft-edge",
    ];
    let mut out = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(home.join(root)) else {
            continue;
        };
        for entry in entries.flatten() {
            let history = entry.path().join("History");
            if history.is_file() {
                out.push(history);
            }
        }
    }
    out
}

/// Collect domain seconds from every supported local browser profile. A locked
/// or malformed profile is skipped; this function never writes to a browser
/// database and never touches the network.
pub fn collect_domains(home: &Path, start: i64, end: i64) -> Vec<(String, i64)> {
    let mut visits = Vec::new();
    for path in firefox_history_paths(home) {
        if let Ok(mut found) = read_firefox_history(&path, start, end) {
            visits.append(&mut found);
        }
    }
    for path in chromium_history_paths(home) {
        if let Ok(mut found) = read_chromium_history(&path, start, end) {
            visits.append(&mut found);
        }
    }
    domain_seconds(&visits, start, end)
}

/// Scan `days` days ending at `today` and replace the stored domain totals for
/// each one. Only called while the user has site names enabled. A day with no
/// visits is cleared, so turning history off or deleting a browser profile
/// cannot leave stale domains behind.
pub fn scan_and_store(store: &Store, home: &Path, today: Day, days: i64) -> Result<usize, String> {
    let days = days.clamp(1, 31);
    let mut updated = 0;
    for offset in (0..days).rev() {
        let day = today.add_days(-offset as i32);
        let totals = collect_domains(home, day.start_ts(), day.end_ts());
        store.replace_domain_totals(day.start_ts(), &totals)?;
        updated += 1;
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daymark-sites-{tag}-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn extracts_only_the_host() {
        assert_eq!(
            domain_from_url("https://www.github.com/slint-ui/slint?tab=readme#top").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            domain_from_url("http://Sub.Example.co.uk:8080/path").as_deref(),
            Some("sub.example.co.uk")
        );
        assert_eq!(
            domain_from_url("https://user:pass@Example.COM./").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn rejects_non_web_and_local_addresses() {
        for url in [
            "file:///tmp/notes.txt",
            "about:blank",
            "chrome://settings",
            "moz-extension://abc/page.html",
            "data:text/html,hello",
            "javascript:alert(1)",
            "https://localhost:3000/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://intranet/",
            "not a url",
            "",
        ] {
            assert_eq!(domain_from_url(url), None, "{url}");
        }
    }

    #[test]
    fn is_case_insensitive_and_strips_a_trailing_dot() {
        assert_eq!(
            domain_from_url("HTTPS://WWW.Example.ORG./x").as_deref(),
            Some("example.org")
        );
    }

    #[test]
    fn firefox_history_is_read_read_only() {
        let dir = TempDir::new("firefox");
        let path = dir.0.join("places.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, hidden INTEGER DEFAULT 0);\n\
                 CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, place_id INTEGER, visit_date INTEGER);\n\
                 INSERT INTO moz_places(id, url) VALUES(1, 'https://github.com/x'), (2, 'https://example.com/y');\n\
                 INSERT INTO moz_historyvisits(place_id, visit_date) VALUES(1, 1000000000), (2, 1060000000);",
            )
            .unwrap();
        drop(connection);
        let visits = read_firefox_history(&path, 0, 2_000).unwrap();
        assert_eq!(visits.len(), 2);
        assert_eq!(visits[0].visited_at, 1_000);
        assert_eq!(visits[1].visited_at, 1_060);
    }

    #[test]
    fn chromium_history_uses_the_1601_epoch() {
        let dir = TempDir::new("chromium");
        let path = dir.0.join("History");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, hidden INTEGER DEFAULT 0);\n\
                 CREATE TABLE visits (id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);\n\
                 INSERT INTO urls(id, url) VALUES(1, 'https://example.com/a');\n\
                 INSERT INTO visits(url, visit_time) VALUES(1, 11644474600000000);",
            )
            .unwrap();
        drop(connection);
        let visits = read_chromium_history(&path, 0, 2_000).unwrap();
        assert_eq!(visits.len(), 1);
        assert_eq!(visits[0].visited_at, 1_000);
    }

    #[test]
    fn domain_seconds_caps_long_gaps_and_sorts_by_time() {
        let visits = vec![
            HistoryVisit {
                url: "https://github.com/a".into(),
                visited_at: 0,
            },
            HistoryVisit {
                url: "https://example.com/b".into(),
                visited_at: 3_600,
            },
            HistoryVisit {
                url: "https://github.com/c".into(),
                visited_at: 3_700,
            },
        ];
        let totals = domain_seconds(&visits, 0, 4_000);
        assert_eq!(
            totals,
            vec![
                ("github.com".into(), 1_800 + 300),
                ("example.com".into(), 100)
            ]
        );
    }

    #[test]
    fn finds_firefox_and_chromium_profile_paths() {
        let home = TempDir::new("home");
        let firefox = home.0.join(".mozilla/firefox/abc.default");
        fs::create_dir_all(&firefox).unwrap();
        fs::write(firefox.join("places.sqlite"), "").unwrap();
        let chromium = home.0.join(".config/chromium/Default");
        fs::create_dir_all(&chromium).unwrap();
        fs::write(chromium.join("History"), "").unwrap();

        assert_eq!(
            firefox_history_paths(&home.0),
            vec![firefox.join("places.sqlite")]
        );
        assert_eq!(
            chromium_history_paths(&home.0),
            vec![chromium.join("History")]
        );
    }

    #[test]
    fn collects_domains_from_both_browser_families() {
        let home = TempDir::new("collect");
        let firefox = home.0.join(".mozilla/firefox/p.default");
        fs::create_dir_all(&firefox).unwrap();
        let connection = Connection::open(firefox.join("places.sqlite")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, hidden INTEGER DEFAULT 0);\n\
                 CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, place_id INTEGER, visit_date INTEGER);\n\
                 INSERT INTO moz_places(id, url) VALUES(1, 'https://github.com/x');\n\
                 INSERT INTO moz_historyvisits(place_id, visit_date) VALUES(1, 1000000000);",
            )
            .unwrap();
        drop(connection);

        let chromium = home.0.join(".config/chromium/Default");
        fs::create_dir_all(&chromium).unwrap();
        let connection = Connection::open(chromium.join("History")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, hidden INTEGER DEFAULT 0);\n\
                 CREATE TABLE visits (id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);\n\
                 INSERT INTO urls(id, url) VALUES(1, 'https://example.com/y');\n\
                 INSERT INTO visits(url, visit_time) VALUES(1, 11644474700000000);",
            )
            .unwrap();
        drop(connection);

        let domains = collect_domains(&home.0, 0, 2_000);
        assert!(domains.iter().any(|(domain, _)| domain == "github.com"));
        assert!(domains.iter().any(|(domain, _)| domain == "example.com"));
    }

    #[test]
    fn scan_and_store_replaces_the_days_domains() {
        let home = TempDir::new("scan-home");
        let firefox = home.0.join(".mozilla/firefox/p.default");
        fs::create_dir_all(&firefox).unwrap();
        let connection = Connection::open(firefox.join("places.sqlite")).unwrap();
        let today = Day::today();
        let micros = (today.start_ts() + 3_600) * 1_000_000;
        connection
            .execute_batch(&format!(
                "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, hidden INTEGER DEFAULT 0);\n\
                 CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, place_id INTEGER, visit_date INTEGER);\n\
                 INSERT INTO moz_places(id, url) VALUES(1, 'https://github.com/x');\n\
                 INSERT INTO moz_historyvisits(place_id, visit_date) VALUES(1, {micros});"
            ))
            .unwrap();
        drop(connection);

        let store = Store::open_at(&home.0.join("daymark.db")).unwrap();
        let updated = scan_and_store(&store, &home.0, today, 1).unwrap();
        assert_eq!(updated, 1);
        assert!(
            store
                .domain_totals_for_day(today.start_ts())
                .unwrap()
                .iter()
                .any(|(domain, _)| domain == "github.com")
        );
    }
}
