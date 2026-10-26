//! Weekly digest: the previous week's total, the same week's top applications,
//! and the change from the week before. Pure data and text; the notification
//! transport lives outside this module.

use std::collections::HashMap;
use std::path::Path;

use crate::date::{self, Day};
use crate::db::Store;
use crate::stats;

/// A completed week, oldest day first.
#[derive(Clone, Debug, PartialEq)]
pub struct Digest {
    pub week_start: Day,
    pub week_end: Day,
    pub total: i64,
    pub previous_total: i64,
    /// Top applications by recorded seconds, largest first.
    pub top: Vec<(String, i64)>,
}

/// Build the digest for the seven days ending at `week_end` (inclusive).
/// Returns `None` when that week has no recorded time, so the app never sends
/// an empty notification.
pub fn build(store: &Store, week_end: Day) -> Result<Option<Digest>, String> {
    let week_start = week_end.add_days(-6);
    let previous_end = week_start.add_days(-1);
    let previous_start = previous_end.add_days(-6);
    let rows = store.sessions_between(previous_start.start_ts(), week_end.end_ts())?;

    let total = (0..7)
        .map(|offset| stats::total_for_day(&rows, week_start.add_days(offset)))
        .sum::<i64>();
    if total == 0 {
        return Ok(None);
    }
    let previous_total = (0..7)
        .map(|offset| stats::total_for_day(&rows, previous_start.add_days(offset)))
        .sum::<i64>();

    let mut by_app: HashMap<i64, i64> = HashMap::new();
    for offset in 0..7 {
        let aggregate = stats::aggregate_day(&rows, week_start.add_days(offset));
        for (app_id, seconds) in aggregate.apps {
            *by_app.entry(app_id).or_insert(0) += seconds;
        }
    }
    let mut ranked: Vec<(i64, i64)> = by_app.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(5);
    let ids: Vec<i64> = ranked.iter().map(|(id, _)| *id).collect();
    let infos = store.app_infos(&ids);
    let top = ranked
        .into_iter()
        .filter_map(|(id, seconds)| {
            infos
                .get(&id)
                .map(|info| (info.name.clone(), seconds))
                .filter(|(_, seconds)| *seconds > 0)
        })
        .collect();

    Ok(Some(Digest {
        week_start,
        week_end,
        total,
        previous_total,
        top,
    }))
}

/// The Sunday that ends the most recent completed week, when a digest is due.
/// The digest is due once a week; `last_sent` stores the last week end that was
/// delivered. A missed Monday is caught up on the next launch.
pub fn due_week_end(today: Day, last_sent: Option<Day>) -> Option<Day> {
    let weekday = today.weekday();
    let days_since_monday = if weekday == 0 { 6 } else { weekday - 1 };
    let monday = today.add_days(-(days_since_monday as i32));
    let week_end = monday.add_days(-1);
    if last_sent.is_some_and(|sent| sent >= week_end) {
        None
    } else {
        Some(week_end)
    }
}

/// Notification title and body. Kept short enough for a GNOME notification and
/// free of titles, URLs or application content.
pub fn summary(digest: &Digest) -> (String, String) {
    let title = format!("Your week: {}", date::format_duration(digest.total));
    let mut body = format!(
        "{} across {} apps.",
        date::format_duration(digest.total),
        digest.top.len()
    );
    if !digest.top.is_empty() {
        let top = digest
            .top
            .iter()
            .take(3)
            .map(|(name, seconds)| format!("{name} {}", date::format_duration(*seconds)))
            .collect::<Vec<_>>()
            .join(", ");
        body.push_str(&format!(" Top: {top}."));
    }
    if digest.previous_total > 0 {
        let delta = digest.total - digest.previous_total;
        let direction = if delta >= 0 { "more" } else { "less" };
        body.push_str(&format!(
            " {} {} than the week before.",
            date::format_delta(delta),
            direction
        ));
    }
    (title, body)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// The local notification payload. The companion watches `notify.json` and
/// shows it through GNOME's own notification system; this module never opens a
/// socket or calls the network.
pub fn notification_payload(digest: &Digest) -> String {
    let (title, body) = summary(digest);
    format!(
        "{{\"id\":{},\"title\":{},\"body\":{}}}\n",
        json_string(&digest.week_end.iso()),
        json_string(&title),
        json_string(&body)
    )
}

/// Write the notification request into a runtime directory, atomically and
/// readable only by the current user.
pub fn write_notification_at(directory: &Path, digest: &Digest) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("could not create {}: {e}", directory.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700));
    }
    let temporary = directory.join("notify.json.tmp");
    let path = directory.join("notify.json");
    std::fs::write(&temporary, notification_payload(digest).as_bytes())
        .map_err(|e| format!("could not write the notification: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&temporary, &path)
        .map_err(|e| format!("could not publish the notification: {e}"))
}

/// Write the notification request into Daymark's runtime directory.
pub fn write_notification(digest: &Digest) -> Result<(), String> {
    write_notification_at(&crate::db::runtime_dir(), digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;
    use crate::desktop::ResolvedApp;
    use std::fs;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daymark-digest-{tag}-{}-{:?}",
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
            name: key.into(),
            icon: None,
            desktop_id: None,
            wm_class: None,
            matched: false,
        }
    }

    fn seed(store: &Store, day: Day, key: &str, seconds: i64) -> i64 {
        let id = store.upsert_app(&app(key), day.start_ts()).unwrap();
        let start = day.start_ts() + 9 * 3600;
        let session = store.open_session(id, start).unwrap();
        store.close_session(session, start + seconds).unwrap();
        id
    }

    #[test]
    fn a_week_digest_ranks_apps_and_compares_the_previous_week() {
        let dir = TempDir::new("week");
        let store = Store::open_at(&dir.db()).unwrap();
        let end = Day::today();
        let start = end.add_days(-6);
        seed(&store, start, "Editor", 2 * 3600);
        seed(&store, start.add_days(1), "Browser", 4 * 3600);
        seed(&store, end, "Editor", 3600);
        seed(&store, start.add_days(-1), "Browser", 3600);

        let digest = build(&store, end).unwrap().unwrap();
        assert_eq!(digest.week_start, start);
        assert_eq!(digest.week_end, end);
        assert_eq!(digest.total, 7 * 3600);
        assert_eq!(digest.previous_total, 3600);
        assert_eq!(digest.top[0], ("Browser".into(), 4 * 3600));
        assert_eq!(digest.top[1], ("Editor".into(), 3 * 3600));
    }

    #[test]
    fn an_empty_week_has_no_digest() {
        let dir = TempDir::new("empty");
        let store = Store::open_at(&dir.db()).unwrap();
        assert!(build(&store, Day::today()).unwrap().is_none());
    }

    #[test]
    fn the_digest_is_due_once_for_the_last_completed_week() {
        // Monday: the completed week ended yesterday (Sunday).
        let monday = Day::new(2026, 10, 26);
        assert_eq!(monday.weekday(), 1);
        let sunday = monday.add_days(-1);
        assert_eq!(due_week_end(monday, None), Some(sunday));
        assert_eq!(due_week_end(monday, Some(sunday)), None);
        assert_eq!(
            due_week_end(monday.add_days(2), Some(sunday)),
            None,
            "the same week is not sent twice"
        );
        assert_eq!(
            due_week_end(monday.add_days(7), Some(sunday)),
            Some(monday.add_days(6)),
            "a missed week is caught up"
        );
    }

    #[test]
    fn the_summary_uses_the_total_top_apps_and_delta() {
        let digest = Digest {
            week_start: Day::today().add_days(-6),
            week_end: Day::today(),
            total: 6 * 3600,
            previous_total: 4 * 3600,
            top: vec![("Browser".into(), 3 * 3600), ("Editor".into(), 2 * 3600)],
        };
        let (title, body) = summary(&digest);
        assert!(title.contains("6h"));
        assert!(body.contains("Browser 3h"));
        assert!(body.contains("2h more than the week before"));
    }

    #[test]
    fn a_notification_is_written_atomically() {
        let dir = TempDir::new("notify");
        let runtime = dir.0.join("runtime");
        let digest = Digest {
            week_start: Day::today().add_days(-6),
            week_end: Day::today(),
            total: 6 * 3600,
            previous_total: 0,
            top: vec![("A \"quoted\" app".into(), 3 * 3600)],
        };
        write_notification_at(&runtime, &digest).unwrap();
        assert!(!runtime.join("notify.json.tmp").exists());
        let text = fs::read_to_string(runtime.join("notify.json")).unwrap();
        let value = crate::json::parse(&text).unwrap();
        assert_eq!(
            value.get("id").and_then(|v| v.as_str()),
            Some(digest.week_end.iso().as_str())
        );
        assert!(
            value
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap()
                .contains("6h")
        );
        assert!(
            value
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap()
                .contains("A \"quoted\" app")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(runtime.join("notify.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
