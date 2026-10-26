//! Pure aggregation of stored intervals into days, hours and per-application
//! totals. Kept independent of SQLite and of the UI so it can be exhaustively
//! tested.

use crate::date::{self, Day};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct SessionRow {
    pub app_id: i64,
    pub started: i64,
    pub ended: i64,
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
}
