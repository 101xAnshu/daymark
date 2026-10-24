//! Local calendar handling without a date-time dependency.
//!
//! Everything stored in the database is a Unix timestamp in seconds. Days are
//! converted through the C library's local-time routines so DST and timezone
//! changes are handled by the same code the rest of the desktop uses.

use std::time::{SystemTime, UNIX_EPOCH};

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

fn local_tm(ts: i64) -> libc::tm {
    // SAFETY: localtime_r only writes to the provided tm and the time value is
    // a valid, owned local.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let t = ts as libc::time_t;
        libc::localtime_r(&t, &mut tm);
        tm
    }
}

fn make_ts(tm: &mut libc::tm) -> i64 {
    tm.tm_isdst = -1;
    // SAFETY: tm is a fully initialized local value.
    unsafe { libc::mktime(tm) as i64 }
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Day {
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    pub fn today() -> Self {
        Self::from_timestamp(unix_now())
    }

    pub fn from_timestamp(ts: i64) -> Self {
        let tm = local_tm(ts);
        Self {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
        }
    }

    /// Unix timestamp of local midnight at the start of this day.
    pub fn start_ts(self) -> i64 {
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        tm.tm_year = self.year - 1900;
        tm.tm_mon = self.month as i32 - 1;
        tm.tm_mday = self.day as i32;
        tm.tm_hour = 0;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        tm.tm_isdst = -1;
        make_ts(&mut tm)
    }

    /// Unix timestamp of local midnight at the start of the following day.
    pub fn end_ts(self) -> i64 {
        self.add_days(1).start_ts()
    }

    pub fn add_days(self, delta: i32) -> Self {
        let noon = self.start_ts() + 12 * 3600;
        let mut tm = local_tm(noon);
        tm.tm_mday += delta;
        let shifted = make_ts(&mut tm);
        Self::from_timestamp(shifted)
    }

    pub fn weekday(self) -> u32 {
        let tm = local_tm(self.start_ts() + 12 * 3600);
        tm.tm_wday as u32
    }

    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// "Thursday, 24 October"
    pub fn pretty(self) -> String {
        format!(
            "{}, {} {}",
            WEEKDAYS[self.weekday() as usize],
            self.day,
            MONTHS[(self.month - 1) as usize]
        )
    }

    /// "Thu 24 Oct"
    pub fn short(self) -> String {
        format!(
            "{} {} {}",
            WEEKDAYS_SHORT[self.weekday() as usize],
            self.day,
            MONTHS_SHORT[(self.month - 1) as usize]
        )
    }

    pub fn weekday_short(self) -> String {
        WEEKDAYS_SHORT[self.weekday() as usize].to_string()
    }

    pub fn weekday_name(self) -> String {
        WEEKDAYS[self.weekday() as usize].to_string()
    }

    /// "24 Oct", without the weekday.
    pub fn day_month(self) -> String {
        format!("{} {}", self.day, MONTHS_SHORT[(self.month - 1) as usize])
    }

    pub fn day_label(self) -> String {
        self.day.to_string()
    }

    /// Human label used in history lists.
    pub fn relative(&self, today: Day) -> String {
        if *self == today {
            "Today".into()
        } else if *self == today.add_days(-1) {
            "Yesterday".into()
        } else {
            self.short()
        }
    }
}

pub fn local_hour(ts: i64) -> u32 {
    local_tm(ts).tm_hour as u32
}

/// Number of seconds in the local day containing `ts` (23, 24 or 25 hours).
pub fn local_day_seconds(ts: i64) -> i64 {
    let day = Day::from_timestamp(ts);
    (day.end_ts() - day.start_ts()).max(60)
}

/// Format a duration the way the interface shows it: "6h 42m", "18m", "<1m".
pub fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds == 0 {
        return "0m".into();
    }
    if seconds < 60 {
        return "<1m".into();
    }
    let minutes = (seconds + 30) / 60;
    let hours = minutes / 60;
    let minutes = minutes % 60;
    match (hours, minutes) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Compact duration used inside tooltips: "6:42" hours:minutes, or "42m".
pub fn format_compact(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let minutes = (seconds + 30) / 60;
    if minutes >= 60 {
        format!("{}h {:02}m", minutes / 60, minutes % 60)
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "<1m".into()
    }
}

/// Rounded split of `total` into a human phrase. Used for comparisons.
pub fn format_delta(seconds: i64) -> String {
    let minutes = ((seconds.abs() + 30) / 60).max(1);
    if minutes >= 60 {
        let h = minutes / 60;
        let m = minutes % 60;
        if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m}m")
        }
    } else {
        format!("{minutes}m")
    }
}

pub fn format_number(n: i64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(0), "0m");
        assert_eq!(format_duration(30), "<1m");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(59 * 60), "59m");
        assert_eq!(format_duration(60 * 60), "1h");
        assert_eq!(format_duration(6 * 3600 + 42 * 60), "6h 42m");
        assert_eq!(format_delta(90), "2m");
        assert_eq!(format_delta(-3660), "1h 1m");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn every_day_roundtrips_and_is_unique() {
        let mut day = Day::today().add_days(-400);
        let mut seen = HashSet::new();
        for _ in 0..800 {
            let start = day.start_ts();
            let end = day.end_ts();
            assert!(start < end, "empty day {day:?}");
            assert_eq!(Day::from_timestamp(start), day, "start of {day:?}");
            assert_eq!(Day::from_timestamp(start + 60), day, "inside {day:?}");
            assert_eq!(Day::from_timestamp(end - 60), day, "end of {day:?}");
            assert!(seen.insert(day), "duplicate day {day:?}");
            assert_eq!(day.add_days(1), Day::from_timestamp(end));
            day = day.add_days(1);
        }
    }

    #[test]
    fn midnight_boundary_splits_intervals() {
        let today = Day::today();
        let yesterday = today.add_days(-1);
        let boundary = today.start_ts();
        assert!(yesterday.end_ts() == boundary);
        let session_start = boundary - 30 * 60;
        let session_end = boundary + 45 * 60;
        assert!(session_start < yesterday.end_ts());
        assert!(session_end > today.start_ts());
    }

    #[test]
    fn relative_labels() {
        let today = Day::today();
        assert_eq!(today.relative(today), "Today");
        assert_eq!(today.add_days(-1).relative(today), "Yesterday");
        assert_ne!(today.add_days(-2).relative(today), "Today");
    }
}
