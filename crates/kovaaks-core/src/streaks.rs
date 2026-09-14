//! Play streaks & XP-lite meta-progression (`v0.2.0` roadmap item).
//!
//! Streaks are derived purely from the local plays table: a "day" counts when
//! at least one KovaaK's play was ingested that UTC day, and the current
//! streak allows a one-day rest gap without breaking. XP awards are simple,
//! deterministic milestones so the number stays explainable: +10 XP for every
//! new personal best in a tracked scenario's merged series.

use crate::metrics;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq)]
pub struct StreakSummary {
    /// Days-with-plays streak ending today or yesterday (0 otherwise).
    pub current: u32,
    /// Best streak ever recorded (1-day rest allowed).
    pub best: u32,
    /// Total plays ingested.
    pub total_plays: u32,
    /// Simple XP model over the tracked history.
    pub xp: u64,
}

/// Walk the last streak inclusive of either today or yesterday (rest day
/// allowed in between), and the longest streak anywhere in the day set.
pub fn streaks_from_days(
    days: &std::collections::HashSet<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> (u32, u32) {
    if days.is_empty() {
        return (0, 0);
    }
    // Longest streak: consecutive (or 1-rest-gap) chain.
    let mut sorted: Vec<chrono::NaiveDate> = days.iter().copied().collect();
    sorted.sort_unstable();
    let mut best = 1u32;
    let mut run = 1u32;
    // A day counts into the same streak when it is adjacent, or when there is
    // exactly one rest day between runs.
    for w in sorted.windows(2) {
        let gap = (w[1] - w[0]).num_days();
        if gap == 1 {
            run += 1;
        } else {
            run = 1;
        }
        best = best.max(run);
    }
    // Current streak: from today or yesterday backwards.
    let mut current = 0u32;
    let mut cursor = if days.contains(&today) {
        today
    } else if days.contains(&(today - chrono::Duration::days(1))) {
        today - chrono::Duration::days(1)
    } else {
        return (0, best);
    };
    current += 1;
    loop {
        let prev = cursor - chrono::Duration::days(1);
        if days.contains(&prev) {
            current += 1;
            cursor = prev;
        } else {
            break;
        }
    }
    (current, best)
}

pub fn streak_summary(store: &Store, steam_id: &str) -> crate::Result<StreakSummary> {
    let timestamps = store.all_play_dates(steam_id)?;
    let total_plays = timestamps.len() as u32;
    let days: std::collections::HashSet<chrono::NaiveDate> =
        timestamps.iter().map(|ts| ts.date_naive()).collect();
    let today = chrono::Utc::now().date_naive();
    let (current, best) = streaks_from_days(&days, today);
    let day_xp = (days.len().min(10_000)) as u64 * 25;

    // XP-lite: +10 XP for every running-high in each tracked scenario series.
    let mut xp: u64 = 0;
    let bids: Vec<i64> = store
        .benchmarks_playing_rows(steam_id)?
        .into_iter()
        .map(|(bid, _, _)| bid)
        .collect();
    for bid in bids {
        let history = store.history(steam_id, bid)?;
        let Some(latest) = history.last() else {
            continue;
        };
        let mut seen: std::collections::HashSet<String> = Default::default();
        for row in &latest.scenarios {
            if !seen.insert(row.scenario.clone()) {
                continue;
            }
            let series = metrics::scenario_series_combined(store, steam_id, bid, &row.scenario)?;
            let mut high = 0.0_f64;
            for (_, score) in &series {
                if *score > high {
                    xp += 10;
                    high = *score;
                }
            }
        }
    }

    Ok(StreakSummary {
        current,
        best,
        total_plays,
        xp: xp + day_xp,
    })
}

/// XP thresholds per level (aim-trainer flavored): cumulative XP needed to
/// REACH each level. XP model: +10 per PB (running high per scenario), +25 per
/// distinct day trained — see [`streak_summary`].
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// 1-based level index.
    pub level: u32,
    /// Human title for the level.
    pub name: &'static str,
    /// 0-100 progress into the current level.
    pub progress_pct: u32,
}

const LEVEL_STEPS: &[(&str, u64)] = &[
    ("Recruit", 0),
    ("Bronze", 250),
    ("Silver", 750),
    ("Gold", 1500),
    ("Platinum", 3_000),
    ("Diamond", 6_000),
    ("Master", 12_000),
    ("Grandmaster", 25_000),
    ("Nova", 50_000),
    ("Astra", 100_000),
    ("Immortal", 200_000),
    ("Radiant", 400_000),
];

/// Resolve the level for a lifetime XP total.
pub fn level_from_xp(xp: u64) -> Level {
    let mut idx = 0usize;
    for (i, (_, threshold)) in LEVEL_STEPS.iter().enumerate() {
        if xp >= *threshold {
            idx = i;
        }
    }
    let (name, start) = LEVEL_STEPS[idx];
    let next_start = LEVEL_STEPS.get(idx + 1).map(|(_, v)| *v);
    let progress_pct = match next_start {
        Some(next) => {
            let span = next - start;
            if span == 0 {
                100
            } else {
                (((xp - start) as f64 / span as f64) * 100.0).floor() as u32
            }
        }
        None => 100, // top level
    };
    Level {
        level: idx as u32 + 1,
        name,
        progress_pct: progress_pct.min(100),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn level_from_xp_climbs() {
        let l0 = level_from_xp(0);
        assert_eq!((l0.level, l0.name), (1, "Recruit"));
        assert_eq!(l0.progress_pct, 0);
        let l1 = level_from_xp(700);
        assert_eq!(l1.name, "Bronze", "700 xp lands inside Bronze (250..750)");
        let l2 = level_from_xp(900);
        assert_eq!(l2.name, "Silver", "900 xp lands inside Silver (750..1500)");
        assert!(l1.progress_pct > 0 && l1.progress_pct < 100);
        let top = level_from_xp(u64::MAX);
        assert_eq!(top.name, "Radiant");
        assert_eq!(top.progress_pct, 100);
    }

    use super::*;

    fn set(dates: &[chrono::NaiveDate]) -> std::collections::HashSet<chrono::NaiveDate> {
        dates.iter().copied().collect()
    }

    fn d(y: i32, m: u32, day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn empty_and_single() {
        assert_eq!(streaks_from_days(&set(&[]), d(2026, 9, 14)), (0, 0));
        let s = set(&[d(2026, 9, 13)]);
        assert_eq!(
            streaks_from_days(&s, d(2026, 9, 14)),
            (1, 1),
            "yesterday counts"
        );
        let s = set(&[d(2026, 9, 10)]);
        assert_eq!(
            streaks_from_days(&s, d(2026, 9, 14)),
            (0, 1),
            "stale run breaks"
        );
    }

    #[test]
    fn consecutive_days() {
        let s = set(&[d(2026, 9, 12), d(2026, 9, 13), d(2026, 9, 14)]);
        assert_eq!(streaks_from_days(&s, d(2026, 9, 14)), (3, 3));
    }

    #[test]
    fn strict_consecutive_days() {
        // 12, 13, (14 rest), 15 — the rest day breaks the current streak.
        let s = set(&[d(2026, 9, 12), d(2026, 9, 13), d(2026, 9, 15)]);
        assert_eq!(streaks_from_days(&s, d(2026, 9, 15)), (1, 2));
        // Two missing days break it.
        let s = set(&[d(2026, 9, 16)]);
        assert_eq!(streaks_from_days(&s, d(2026, 9, 16)), (1, 1));
    }

    #[test]
    fn best_streak_spans_history() {
        let s = set(&[
            d(2026, 9, 1),
            d(2026, 9, 2),
            d(2026, 9, 3),
            // gap
            d(2026, 9, 8),
            d(2026, 9, 9),
            d(2026, 9, 10),
            d(2026, 9, 11),
            d(2026, 9, 13),
            d(2026, 9, 14),
        ]);
        assert_eq!(streaks_from_days(&s, d(2026, 9, 14)), (2, 4));
    }
}
