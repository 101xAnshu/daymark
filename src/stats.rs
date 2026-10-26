//! Pure aggregation of stored intervals into days, hours and per-application
//! totals. Kept independent of SQLite and of the UI so it can be exhaustively
//! tested.

use crate::date::{self, Day};
use std::collections::{HashMap, HashSet};

/// The early hours counted as "after midnight" in the day metrics. Kept small
/// and explicit so the interface can label the same window it measures.
pub const AFTER_MIDNIGHT_HOURS: i64 = 5;

#[derive(Clone, Copy, Debug)]
pub struct SessionRow {
    pub app_id: i64,
    pub started: i64,
    pub ended: i64,
}

/// A day's preserved per-application total. Raw intervals are folded into these
/// rows before retention deletes them, so the day stays answerable without
/// carrying hourly detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollupTotal {
    pub day_start: i64,
    pub app_id: i64,
    pub seconds: i64,
}

/// Raw intervals and preserved daily totals for the same date range. A day is
/// answered from at most one source: raw intervals win for a day that still has
/// them, so a day present in both places is never counted twice.
#[derive(Clone, Debug, Default)]
pub struct DayData {
    pub rows: Vec<SessionRow>,
    pub rollups: Vec<RollupTotal>,
}

/// The part of a day the interface can honestly show. `has_intervals` decides
/// whether the timeline and hourly chart have data at all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DayTotals {
    pub total: i64,
    pub apps: Vec<(i64, i64)>,
    pub has_intervals: bool,
    pub has_rollup: bool,
}

impl DayTotals {
    /// A day whose raw intervals aged out. The total and per-app split survive,
    /// but the hour of day does not, so the interface must not invent it.
    pub fn totals_only(&self) -> bool {
        self.has_rollup && !self.has_intervals
    }
}

/// One session on the day timeline, as a fraction of the local day. Fractions
/// rather than pixels keep the UI independent of the window width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineSegment {
    pub app_id: i64,
    pub start: f32,
    pub end: f32,
}

#[derive(Clone, Debug, Default)]
pub struct DayAggregate {
    pub total: i64,
    pub hourly: [i64; 24],
    pub apps: Vec<(i64, i64)>,
}

impl DayAggregate {
    pub fn peak_hour(&self) -> Option<(usize, i64)> {
        let (hour, value) = self.hourly.iter().enumerate().max_by_key(|(_, v)| **v)?;
        if *value <= 0 {
            None
        } else {
            Some((hour, *value))
        }
    }
}

pub fn overlap(a_start: i64, a_end: i64, b_start: i64, b_end: i64) -> i64 {
    let start = a_start.max(b_start);
    let end = a_end.min(b_end);
    (end - start).max(0)
}

pub fn aggregate_day(rows: &[SessionRow], day: Day) -> DayAggregate {
    let day_start = day.start_ts();
    let day_end = day.end_ts();
    let mut out = DayAggregate::default();
    let mut apps: HashMap<i64, i64> = HashMap::new();

    for row in rows {
        if row.ended <= row.started {
            continue;
        }
        let clipped_start = row.started.max(day_start);
        let clipped_end = row.ended.min(day_end);
        if clipped_end <= clipped_start {
            continue;
        }
        let amount = clipped_end - clipped_start;
        out.total += amount;
        *apps.entry(row.app_id).or_insert(0) += amount;

        // Hourly attribution. Stepping a minute keeps local-hour mapping honest
        // across DST transitions and never loops on a missing/duplicated hour.
        let mut t = clipped_start;
        while t < clipped_end {
            let chunk = (clipped_end - t).min(60);
            let hour = date::local_hour(t) as usize;
            if hour < 24 {
                out.hourly[hour] += chunk;
            }
            t += chunk;
        }
    }

    out.apps = apps.into_iter().collect();
    out.apps
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

pub fn total_for_day(rows: &[SessionRow], day: Day) -> i64 {
    let start = day.start_ts();
    let end = day.end_ts();
    rows.iter()
        .map(|r| overlap(r.started, r.ended, start, end))
        .sum()
}

/// Whether the day still has any raw interval to draw.
pub fn day_has_intervals(rows: &[SessionRow], day: Day) -> bool {
    rows.iter().any(|r| clipped_span(r, day).is_some())
}

/// Rollup rows that belong to `day`.
fn rollups_for_day(rollups: &[RollupTotal], day: Day) -> impl Iterator<Item = &RollupTotal> {
    let start = day.start_ts();
    rollups.iter().filter(move |r| r.day_start == start)
}

/// Total and per-app split for one day, drawn from raw intervals when the day
/// still has them and from the preserved rollup otherwise. Never both.
pub fn day_totals(rows: &[SessionRow], rollups: &[RollupTotal], day: Day) -> DayTotals {
    let has_intervals = day_has_intervals(rows, day);
    let mut has_rollup = rollups_for_day(rollups, day).next().is_some();

    if has_intervals {
        let aggregate = aggregate_day(rows, day);
        return DayTotals {
            total: aggregate.total,
            apps: aggregate.apps,
            has_intervals: true,
            has_rollup,
        };
    }

    let mut apps: HashMap<i64, i64> = HashMap::new();
    for rollup in rollups_for_day(rollups, day) {
        *apps.entry(rollup.app_id).or_insert(0) += rollup.seconds;
    }
    let mut apps: Vec<(i64, i64)> = apps.into_iter().filter(|(_, v)| *v > 0).collect();
    apps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    has_rollup = !apps.is_empty();
    let total = apps.iter().map(|(_, seconds)| *seconds).sum();
    DayTotals {
        total,
        apps,
        has_intervals: false,
        has_rollup,
    }
}

pub fn total_with_rollups(rows: &[SessionRow], rollups: &[RollupTotal], day: Day) -> i64 {
    day_totals(rows, rollups, day).total
}

/// Totals for `days` consecutive days ending at `end` (inclusive), oldest
/// first, with each day answered from raw intervals or its rollup.
pub fn totals_ending_at_with_rollups(
    rows: &[SessionRow],
    rollups: &[RollupTotal],
    end: Day,
    days: i64,
) -> Vec<(Day, i64)> {
    (0..days)
        .map(|i| {
            let day = end.add_days(-(days - 1 - i) as i32);
            (day, total_with_rollups(rows, rollups, day))
        })
        .collect()
}

/// Difference from the same weekday one week earlier, using rollups when raw
/// intervals for that day have already aged out.
pub fn week_delta_with_rollups(
    rows: &[SessionRow],
    rollups: &[RollupTotal],
    day: Day,
) -> Option<i64> {
    let previous = total_with_rollups(rows, rollups, day.add_days(-7));
    (previous > 0).then(|| total_with_rollups(rows, rollups, day) - previous)
}

/// The part of a stored interval that falls inside `day`, if any.
fn clipped_span(row: &SessionRow, day: Day) -> Option<(i64, i64)> {
    let start = row.started.max(day.start_ts());
    let end = row.ended.min(day.end_ts());
    (end > start).then_some((start, end))
}

/// Longest unbroken run of recorded activity on `day`, regardless of which
/// application held focus. Contiguous or overlapping sessions join; any idle
/// gap starts a new run. Returns 0 for an empty day.
pub fn longest_stretch(rows: &[SessionRow], day: Day) -> i64 {
    let mut spans: Vec<(i64, i64)> = rows.iter().filter_map(|r| clipped_span(r, day)).collect();
    spans.sort_unstable();
    let Some((mut start, mut end)) = spans.first().copied() else {
        return 0;
    };
    let mut best = end - start;
    for (next_start, next_end) in spans.into_iter().skip(1) {
        if next_start <= end {
            end = end.max(next_end);
        } else {
            start = next_start;
            end = next_end;
        }
        best = best.max(end - start);
    }
    best
}

/// Number of different applications with recorded time on `day`.
pub fn distinct_apps(rows: &[SessionRow], day: Day) -> usize {
    rows.iter()
        .filter(|r| clipped_span(r, day).is_some())
        .map(|r| r.app_id)
        .collect::<HashSet<_>>()
        .len()
}

/// Recorded time between local midnight and [`AFTER_MIDNIGHT_HOURS`] on
/// `day`. A session that straddles midnight contributes only its early-morning
/// portion, and the 05:00 boundary is excluded.
pub fn after_midnight_seconds(rows: &[SessionRow], day: Day) -> i64 {
    let day_start = day.start_ts();
    let window_end = early_morning_end(day);
    rows.iter()
        .map(|r| overlap(r.started, r.ended, day_start, window_end))
        .sum()
}

/// Timestamp of local 05:00 on `day`. Found by stepping through the early
/// hours so a DST transition cannot turn the five-hour label into four or six
/// elapsed hours.
fn early_morning_end(day: Day) -> i64 {
    let day_end = day.end_ts();
    let mut cutoff = day.start_ts();
    while cutoff < day_end && date::local_hour(cutoff) < AFTER_MIDNIGHT_HOURS as u32 {
        cutoff += 60;
    }
    cutoff.min(day_end)
}

/// Difference between `day` and the same weekday one week earlier. `None` when
/// the earlier day has no recorded time, so the interface never compares
/// against an invented zero baseline.
pub fn week_delta(rows: &[SessionRow], day: Day) -> Option<i64> {
    let previous = total_for_day(rows, day.add_days(-7));
    (previous > 0).then(|| total_for_day(rows, day) - previous)
}

/// Sessions clipped to `day` and expressed as fractions of the local day.
/// Touching or overlapping sessions from the same app are merged, then the
/// result is ordered by start time. An empty day yields an empty vector.
pub fn timeline_segments(rows: &[SessionRow], day: Day) -> Vec<TimelineSegment> {
    let day_start = day.start_ts();
    let day_seconds = (day.end_ts() - day_start).max(1) as f32;
    let mut per_app: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
    for row in rows {
        if let Some(span) = clipped_span(row, day) {
            per_app.entry(row.app_id).or_default().push(span);
        }
    }

    let mut segments = Vec::new();
    for (app_id, mut spans) in per_app {
        spans.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::new();
        for (start, end) in spans {
            match merged.last_mut() {
                Some((_, last_end)) if start <= *last_end => *last_end = (*last_end).max(end),
                _ => merged.push((start, end)),
            }
        }
        for (start, end) in merged {
            segments.push(TimelineSegment {
                app_id,
                start: (start - day_start) as f32 / day_seconds,
                end: (end - day_start) as f32 / day_seconds,
            });
        }
    }
    segments.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then_with(|| a.app_id.cmp(&b.app_id))
    });
    segments
}

/// Totals for `days` consecutive days ending at `end` (inclusive), oldest first.
pub fn totals_ending_at(rows: &[SessionRow], end: Day, days: i64) -> Vec<(Day, i64)> {
    (0..days)
        .map(|i| {
            let day = end.add_days(-(days - 1 - i) as i32);
            (day, total_for_day(rows, day))
        })
        .collect()
}

/// Hourly totals for `days` consecutive days ending at `end` (inclusive),
/// oldest first. Each row is a day and each column is a local hour.
pub fn hourly_ending_at(rows: &[SessionRow], end: Day, days: i64) -> Vec<(Day, [i64; 24])> {
    (0..days)
        .map(|i| {
            let day = end.add_days(-(days - 1 - i) as i32);
            (day, aggregate_day(rows, day).hourly)
        })
        .collect()
}

/// Average over the days that actually contain usage. `None` when fewer than
/// three days have data, so the interface never invents a baseline.
pub fn active_day_average(totals: &[(Day, i64)], minimum_days: usize) -> Option<i64> {
    let active: Vec<i64> = totals.iter().map(|(_, v)| *v).filter(|v| *v > 0).collect();
    if active.len() < minimum_days {
        return None;
    }
    Some(active.iter().sum::<i64>() / active.len() as i64)
}

pub fn sum_totals(totals: &[(Day, i64)]) -> i64 {
    totals.iter().map(|(_, v)| *v).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Day;

    fn at(day: Day, hour: i64, minute: i64) -> i64 {
        day.start_ts() + hour * 3600 + minute * 60
    }

    #[test]
    fn overlap_clamps_to_both_ranges() {
        assert_eq!(overlap(10, 20, 15, 25), 5);
        assert_eq!(overlap(10, 20, 0, 5), 0);
        assert_eq!(overlap(10, 20, 30, 40), 0);
        assert_eq!(overlap(0, 100, 20, 30), 10);
    }

    #[test]
    fn sessions_are_attributed_to_each_side_of_midnight() {
        let today = Day::today();
        let yesterday = today.add_days(-1);
        let boundary = today.start_ts();
        let rows = [SessionRow {
            app_id: 1,
            started: boundary - 1800,
            ended: boundary + 2700,
        }];
        assert_eq!(total_for_day(&rows, today), 2700);
        assert_eq!(total_for_day(&rows, yesterday), 1800);
        let split = aggregate_day(&rows, yesterday);
        assert_eq!(split.total, 1800);
        let split = aggregate_day(&rows, today);
        assert_eq!(split.total, 2700);
        assert_eq!(split.hourly.iter().sum::<i64>(), 2700);
    }

    #[test]
    fn per_app_totals_are_sorted_and_exact() {
        let day = Day::today();
        let rows = [
            SessionRow {
                app_id: 1,
                started: at(day, 9, 0),
                ended: at(day, 10, 0),
            },
            SessionRow {
                app_id: 2,
                started: at(day, 10, 0),
                ended: at(day, 11, 30),
            },
            SessionRow {
                app_id: 1,
                started: at(day, 13, 0),
                ended: at(day, 13, 20),
            },
        ];
        let agg = aggregate_day(&rows, day);
        assert_eq!(agg.total, 5400 + 1200 + 3600);
        assert_eq!(agg.apps, vec![(2, 5400), (1, 4800)]);
        assert_eq!(agg.hourly.iter().sum::<i64>(), agg.total);
        let (peak, value) = agg.peak_hour().unwrap();
        assert_eq!(peak, 10);
        assert_eq!(value, 3600);
    }

    #[test]
    fn idle_gaps_are_not_counted() {
        let day = Day::today();
        let rows = [
            SessionRow {
                app_id: 1,
                started: at(day, 8, 0),
                ended: at(day, 8, 30),
            },
            // 8:30 to 10:00: machine idle / locked, nothing stored.
            SessionRow {
                app_id: 1,
                started: at(day, 10, 0),
                ended: at(day, 10, 15),
            },
        ];
        assert_eq!(total_for_day(&rows, day), 45 * 60);
        let agg = aggregate_day(&rows, day);
        assert_eq!(agg.hourly[9], 0);
        assert_eq!(agg.hourly[8], 30 * 60);
        assert_eq!(agg.hourly[10], 15 * 60);
    }

    #[test]
    fn average_needs_real_history() {
        let day = Day::today();
        let totals = totals_ending_at(&[], day, 7);
        assert_eq!(active_day_average(&totals, 3), None);
        let totals = vec![
            (day.add_days(-3), 100),
            (day.add_days(-2), 0),
            (day.add_days(-1), 200),
            (day, 300),
        ];
        assert_eq!(active_day_average(&totals, 3), Some(200));
        assert_eq!(active_day_average(&totals, 4), None);
    }

    #[test]
    fn empty_day_is_zero_not_a_panic() {
        let day = Day::today();
        let agg = aggregate_day(&[], day);
        assert_eq!(agg.total, 0);
        assert_eq!(agg.peak_hour(), None);
        assert_eq!(agg.hourly.iter().sum::<i64>(), 0);
    }

    #[test]
    fn hourly_ending_at_returns_seven_days_oldest_first() {
        let day = Day::today();
        let rows = [SessionRow {
            app_id: 1,
            started: at(day, 9, 30),
            ended: at(day, 10, 15),
        }];
        let heat = hourly_ending_at(&rows, day, 7);
        assert_eq!(heat.len(), 7);
        assert_eq!(heat[0].0, day.add_days(-6));
        assert_eq!(heat[6].0, day);
        assert_eq!(heat[6].1[9], 30 * 60);
        assert_eq!(heat[6].1[10], 15 * 60);
        assert_eq!(heat[6].1.iter().sum::<i64>(), 45 * 60);
        assert_eq!(heat[0].1.iter().sum::<i64>(), 0);
    }

    #[test]
    fn hourly_ending_at_splits_at_midnight() {
        let day = Day::today();
        let next = day.add_days(1);
        let rows = [SessionRow {
            app_id: 1,
            started: at(day, 23, 30),
            ended: at(next, 0, 30),
        }];
        let heat = hourly_ending_at(&rows, next, 2);
        assert_eq!(heat[0].0, day);
        assert_eq!(heat[1].0, next);
        assert_eq!(heat[0].1[23], 30 * 60);
        assert_eq!(heat[1].1[0], 30 * 60);
        assert_eq!(heat[0].1.iter().sum::<i64>(), 30 * 60);
        assert_eq!(heat[1].1.iter().sum::<i64>(), 30 * 60);
    }

    #[test]
    fn longest_stretch_joins_contiguous_sessions_but_not_gaps() {
        let day = Day::today();
        let rows = [
            SessionRow {
                app_id: 1,
                started: at(day, 9, 0),
                ended: at(day, 9, 30),
            },
            // A focus switch with no idle gap is still one stretch.
            SessionRow {
                app_id: 2,
                started: at(day, 9, 30),
                ended: at(day, 10, 15),
            },
            // 45 idle minutes break the run.
            SessionRow {
                app_id: 1,
                started: at(day, 11, 0),
                ended: at(day, 11, 20),
            },
        ];
        assert_eq!(longest_stretch(&rows, day), 75 * 60);
    }

    #[test]
    fn longest_stretch_merges_overlaps_and_clips_midnight() {
        let day = Day::today();
        let yesterday = day.add_days(-1);
        let rows = [
            SessionRow {
                app_id: 1,
                started: at(day, 9, 0),
                ended: at(day, 10, 0),
            },
            SessionRow {
                app_id: 2,
                started: at(day, 9, 30),
                ended: at(day, 10, 30),
            },
        ];
        assert_eq!(longest_stretch(&rows, day), 90 * 60);

        let across = [SessionRow {
            app_id: 1,
            started: at(yesterday, 23, 30),
            ended: at(day, 0, 45),
        }];
        assert_eq!(longest_stretch(&across, yesterday), 30 * 60);
        assert_eq!(longest_stretch(&across, day), 45 * 60);
    }

    #[test]
    fn longest_stretch_of_an_empty_day_is_zero() {
        let day = Day::today();
        assert_eq!(longest_stretch(&[], day), 0);
        let outside = [SessionRow {
            app_id: 1,
            started: at(day.add_days(-1), 9, 0),
            ended: at(day.add_days(-1), 10, 0),
        }];
        assert_eq!(longest_stretch(&outside, day), 0);
    }

    #[test]
    fn distinct_apps_ignores_duplicates_and_other_days() {
        let day = Day::today();
        let rows = [
            SessionRow {
                app_id: 1,
                started: at(day, 9, 0),
                ended: at(day, 9, 30),
            },
            SessionRow {
                app_id: 1,
                started: at(day, 10, 0),
                ended: at(day, 10, 30),
            },
            SessionRow {
                app_id: 2,
                started: at(day, 11, 0),
                ended: at(day, 11, 30),
            },
            SessionRow {
                app_id: 3,
                started: at(day.add_days(-1), 11, 0),
                ended: at(day.add_days(-1), 12, 0),
            },
        ];
        assert_eq!(distinct_apps(&rows, day), 2);
        assert_eq!(distinct_apps(&rows, day.add_days(-1)), 1);
        assert_eq!(distinct_apps(&[], day), 0);
    }

    #[test]
    fn after_midnight_counts_only_the_early_hours() {
        let day = Day::today();
        let yesterday = day.add_days(-1);
        let cutoff = early_morning_end(day);
        assert_eq!(date::local_hour(cutoff), 5);
        let rows = [
            // Straddles midnight: 90 minutes land after 00:00.
            SessionRow {
                app_id: 1,
                started: at(yesterday, 23, 0),
                ended: at(day, 1, 30),
            },
            // Only the half hour before local 05:00 counts.
            SessionRow {
                app_id: 2,
                started: cutoff - 30 * 60,
                ended: cutoff + 3600,
            },
            SessionRow {
                app_id: 3,
                started: at(day, 7, 0),
                ended: at(day, 8, 0),
            },
        ];
        assert_eq!(after_midnight_seconds(&rows, day), 90 * 60 + 30 * 60);
        assert_eq!(after_midnight_seconds(&rows, yesterday), 0);
        assert_eq!(after_midnight_seconds(&[], day), 0);
    }

    #[test]
    fn week_delta_compares_the_same_weekday() {
        let day = Day::today();
        let previous = day.add_days(-7);
        let rows = [
            SessionRow {
                app_id: 1,
                started: at(day, 9, 0),
                ended: at(day, 10, 0),
            },
            SessionRow {
                app_id: 1,
                started: at(previous, 9, 0),
                ended: at(previous, 9, 30),
            },
        ];
        assert_eq!(week_delta(&rows, day), Some(30 * 60));
        assert_eq!(week_delta(&rows, previous), None);
        assert_eq!(week_delta(&[], day), None);
    }

    #[test]
    fn timeline_segments_clip_to_the_day_and_split_midnight() {
        let day = Day::today();
        let yesterday = day.add_days(-1);
        let day_start = day.start_ts();
        let day_seconds = (day.end_ts() - day_start) as f32;
        let rows = [SessionRow {
            app_id: 1,
            started: at(yesterday, 23, 0),
            ended: at(day, 1, 0),
        }];
        let today = timeline_segments(&rows, day);
        assert_eq!(today.len(), 1);
        assert_eq!(today[0].app_id, 1);
        assert!((today[0].start - 0.0).abs() < 0.0001);
        assert!(((today[0].end - today[0].start) * day_seconds - 3600.0).abs() < 1.0);

        let before = timeline_segments(&rows, yesterday);
        assert_eq!(before.len(), 1);
        assert!((before[0].end - 1.0).abs() < 0.0001);
        assert!((((before[0].end - before[0].start) * day_seconds) - 3600.0).abs() < 1.0);

        assert!(timeline_segments(&[], day).is_empty());
    }

    #[test]
    fn timeline_segments_merge_touching_spans_from_one_app() {
        let day = Day::today();
        let rows = [
            SessionRow {
                app_id: 1,
                started: at(day, 9, 0),
                ended: at(day, 9, 30),
            },
            SessionRow {
                app_id: 2,
                started: at(day, 9, 45),
                ended: at(day, 10, 15),
            },
            SessionRow {
                app_id: 1,
                started: at(day, 9, 30),
                ended: at(day, 10, 0),
            },
        ];
        let segments = timeline_segments(&rows, day);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].app_id, 1);
        assert!((segments[0].start - segments[1].start).abs() > 0.0001);
        let day_seconds = (day.end_ts() - day.start_ts()) as f32;
        let first_minutes = (segments[0].end - segments[0].start) * day_seconds / 60.0;
        assert!((first_minutes - 60.0).abs() < 1.0);
        assert_eq!(segments[1].app_id, 2);
    }

    fn rollup(day: Day, app_id: i64, seconds: i64) -> RollupTotal {
        RollupTotal {
            day_start: day.start_ts(),
            app_id,
            seconds,
        }
    }

    #[test]
    fn a_rolled_up_day_keeps_its_total_and_apps_without_intervals() {
        let day = Day::today();
        let rollups = [rollup(day, 1, 3600), rollup(day, 2, 1800)];
        let totals = day_totals(&[], &rollups, day);
        assert_eq!(totals.total, 5400);
        assert_eq!(totals.apps, vec![(1, 3600), (2, 1800)]);
        assert!(!totals.has_intervals);
        assert!(totals.has_rollup);
        assert!(totals.totals_only());
    }

    #[test]
    fn raw_intervals_win_over_a_rollup_for_the_same_day() {
        let day = Day::today();
        let rows = [SessionRow {
            app_id: 1,
            started: at(day, 9, 0),
            ended: at(day, 9, 30),
        }];
        let rollups = [rollup(day, 1, 10 * 3600), rollup(day, 2, 3600)];
        let totals = day_totals(&rows, &rollups, day);
        assert_eq!(totals.total, 1800);
        assert_eq!(totals.apps, vec![(1, 1800)]);
        assert!(totals.has_intervals);
        assert!(!totals.totals_only());
    }

    #[test]
    fn an_old_empty_day_is_zero_and_not_totals_only() {
        let day = Day::today().add_days(-400);
        let totals = day_totals(&[], &[], day);
        assert_eq!(totals.total, 0);
        assert!(totals.apps.is_empty());
        assert!(!totals.has_intervals);
        assert!(!totals.has_rollup);
        assert!(!totals.totals_only());
    }

    #[test]
    fn totals_ending_at_choose_the_source_per_day() {
        let day = Day::today();
        let rolled = day.add_days(-3);
        let raw = day.add_days(-1);
        let empty = day.add_days(-2);
        let rows = [SessionRow {
            app_id: 1,
            started: at(raw, 9, 0),
            ended: at(raw, 10, 0),
        }];
        let rollups = [rollup(rolled, 2, 45 * 60)];
        let totals = totals_ending_at_with_rollups(&rows, &rollups, day, 4);
        assert_eq!(
            totals,
            vec![(rolled, 45 * 60), (empty, 0), (raw, 3600), (day, 0),]
        );
    }

    #[test]
    fn week_delta_reads_the_rolled_up_weekday() {
        let day = Day::today();
        let previous = day.add_days(-7);
        let rollups = [rollup(previous, 1, 20 * 60), rollup(day, 1, 35 * 60)];
        assert_eq!(week_delta_with_rollups(&[], &rollups, day), Some(15 * 60));
        assert_eq!(week_delta_with_rollups(&[], &[], day), None);
    }
}
