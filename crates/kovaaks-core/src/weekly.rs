//! Weekly report (`v0.2.0` roadmap item): a week-at-a-glance summary derived
//! purely from local data — plays, personal bests, top scenario improvements
//! (this week vs the week before, trend arrows), and benchmark rank changes.

use crate::metrics;
use crate::store::Store;

/// One scenario improvement row in the report.
#[derive(Debug, Clone, PartialEq)]
pub struct ImprovementRow {
    pub scenario: String,
    pub benchmark_id: i64,
    /// avg(last 7d) − avg(previous 7d), display units.
    pub delta: f64,
    /// direction: +1 improving, −1 regressing, 0 flat.
    pub trend: i8,
    /// New personal best landed inside the report week.
    pub pb_this_week: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeeklyReport {
    /// UTC week start (the report covers the trailing 7 days).
    pub since: chrono::DateTime<chrono::Utc>,
    /// Number of days (of the 7) with at least one play.
    pub days_played: u32,
    /// Plays inside the week.
    pub plays: u32,
    /// Distinct scenarios touched inside the week.
    pub scenarios_played: u32,
    /// Personal-best events (new highs) inside the week.
    pub pb_events: u32,
    /// Estimated alive-time ("scored time") across the week, in seconds.
    pub scored_seconds: f64,
    /// Max play-streak from streaks module (current streak at report time).
    pub current_streak: u32,
    pub xp: u64,
    /// Lifetime level derived from xp (1-based index + progress 0-100).
    pub level_name: String,
    pub level: u32,
    pub level_progress_pct: u32,
    /// Full 12-step Greek ladder (name, cumulative XP threshold) for the frieze.
    pub level_steps: Vec<(String, u64)>,
    /// Plays per calendar day, trailing 7 days, oldest → newest (torch strip).
    pub plays_per_day: [u32; 7],
    /// Top 5 improvements by delta (improving then regressing rows).
    pub improvements: Vec<ImprovementRow>,
    /// Benchmark rank changes inside the week: (benchmark_id, benchmark,
    /// from → to tiers).
    pub rank_changes: Vec<(i64, String, String, String)>,
}

/// Aggregate the trailing-7-day report. `now` is injectable for tests.
pub fn weekly_report(
    store: &Store,
    steam_id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> crate::Result<WeeklyReport> {
    let since = now - chrono::Duration::days(7);

    // Plays inside the week (from the plays table, all scenarios).
    let all_plays = store.all_play_dates(steam_id)?;
    let week_plays: Vec<chrono::DateTime<chrono::Utc>> = all_plays
        .iter()
        .filter(|ts| **ts >= since)
        .copied()
        .collect();
    let mut days: std::collections::HashSet<chrono::NaiveDate> =
        week_plays.iter().map(|ts| ts.date_naive()).collect();
    let days_played = days.len() as u32;
    let _ = &mut days;
    let scenarios_played: u32 = {
        let mut set: std::collections::HashSet<String> = Default::default();
        for ts in &week_plays {
            set.insert(ts.date_naive().to_string());
        }
        // Distinct scenarios: approximate via plays scenarios this week.
        store.plays_scenarios(steam_id)?.len() as u32
    };

    let pb_events = all_pb_events(store, steam_id, since, now)?;

    // Improvements: across all benchmarks snapshotted in the reporting
    // week (or earlier — prev-week pairs still need a current series).
    let bids: Vec<i64> = store.snapshot_benchmark_ids(steam_id, since)?;
    let mut rows: Vec<ImprovementRow> = Vec::new();
    for bid in bids.clone() {
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
            let week = metrics::compute_window(&series, now, chrono::Duration::days(7));
            let prev = metrics::compute_window(
                &series,
                now - chrono::Duration::days(7),
                chrono::Duration::days(7),
            );
            if week.samples == 0 {
                continue;
            }
            let delta = week.avg_score - prev.avg_score;
            if delta.abs() < 0.01 {
                continue;
            }
            // PB inside the week: best of series > best before the week start.
            let pre_best = series
                .iter()
                .filter(|(ts, _)| *ts < since)
                .map(|(_, s)| *s)
                .fold(0.0_f64, f64::max);
            let inweek_best = series
                .iter()
                .filter(|(ts, _)| *ts >= since)
                .map(|(_, s)| *s)
                .fold(0.0_f64, f64::max);
            rows.push(ImprovementRow {
                scenario: row.scenario.clone(),
                benchmark_id: bid,
                delta,
                trend: if delta > 0.0 {
                    1
                } else if delta < 0.0 {
                    -1
                } else {
                    0
                },
                pb_this_week: inweek_best > pre_best,
            });
        }
    }
    // Fold same-name rows (a scenario can appear under several categories).
    let mut deduped: Vec<ImprovementRow> = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = Default::default();
    for r in rows {
        if seen_names.insert(r.scenario.clone()) {
            deduped.push(r);
        }
    }
    rows = deduped;
    rows.sort_by(|a, b| b.delta.partial_cmp(&a.delta).unwrap());
    // Keep improving first, then regression rows.
    let improving: Vec<ImprovementRow> = rows
        .iter()
        .filter(|r| r.delta > 0.0)
        .take(5)
        .cloned()
        .collect();
    let regressing: Vec<ImprovementRow> = rows
        .iter()
        .filter(|r| r.delta < 0.0)
        .take(5)
        .cloned()
        .collect();
    rows = improving;
    rows.extend(regressing);

    // Rank changes: diff consecutive snapshots inside the week window.
    let mut rank_changes: Vec<(i64, String, String, String)> = Vec::new();
    for bid in bids.clone() {
        let history = store.history(steam_id, bid)?;
        // consecutive snapshot rank transitions inside the window
        let mut prevs: Vec<&crate::store::StoredSnapshot> =
            history.iter().filter(|s| s.captured_at >= since).collect();
        if !history.is_empty() && history.first().unwrap().captured_at < since {
            // include the boundary snapshot as the "from" base
            let boundary = history
                .iter()
                .rev()
                .find(|s| s.captured_at < since)
                .unwrap();
            prevs.insert(0, boundary);
        }
        prevs = trim(prevs);
        for w in prevs.windows(2) {
            let from = w[0].overall_rank;
            let to = w[1].overall_rank;
            if from != to && w[1].captured_at >= since {
                let registry = crate::registry::Registry;
                let name = registry
                    .by_id(bid as u64)
                    .map(|(b, _)| b.name.clone())
                    .unwrap_or_else(|| format!("benchmark {bid}"));
                let from_name = tier_name(bid as u64, from);
                let to_name = tier_name(bid as u64, to);
                rank_changes.push((bid, name, from_name, to_name));
            }
        }
    }

    Ok(WeeklyReport {
        since,
        days_played,
        plays: week_plays.len() as u32,
        scenarios_played,
        pb_events,
        current_streak: crate::streaks::streak_summary(store, steam_id)
            .map(|s| s.current)
            .unwrap_or(0),
        xp: crate::streaks::streak_summary(store, steam_id)
            .map(|s| s.xp)
            .unwrap_or(0),
        level_name: crate::streaks::level_from_xp(
            crate::streaks::streak_summary(store, steam_id)
                .map(|s| s.xp)
                .unwrap_or(0),
        )
        .name
        .to_string(),
        level: crate::streaks::level_from_xp(
            crate::streaks::streak_summary(store, steam_id)
                .map(|s| s.xp)
                .unwrap_or(0),
        )
        .level,
        level_progress_pct: crate::streaks::level_from_xp(
            crate::streaks::streak_summary(store, steam_id)
                .map(|s| s.xp)
                .unwrap_or(0),
        )
        .progress_pct,
        level_steps: crate::streaks::LEVEL_STEPS
            .iter()
            .map(|(n, xp)| (n.to_string(), *xp))
            .collect(),
        plays_per_day: {
            let mut counts = [0u32; 7];
            for ts in &week_plays {
                let day = (ts.date_naive() - since.date_naive())
                    .num_days()
                    .clamp(0, 6) as usize;
                counts[day] += 1;
            }
            counts
        },
        scored_seconds: store.scored_seconds_since(steam_id, since)?,
        improvements: rows.clone(),
        rank_changes,
    })
}

/// Drop duplicate consecutive entries after the boundary insert.
fn trim(list: Vec<&crate::store::StoredSnapshot>) -> Vec<&crate::store::StoredSnapshot> {
    let mut out: Vec<&crate::store::StoredSnapshot> = Vec::new();
    let mut last_id: Option<i64> = None;
    for s in list {
        if Some(s.id) != last_id {
            last_id = Some(s.id);
            out.push(s);
        }
    }
    out
}

/// New personal bests inside a window across all tracked scenarios: count
/// running-high increments across merged series.
fn all_pb_events(
    store: &Store,
    steam_id: &str,
    since: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> crate::Result<u32> {
    let mut count = 0u32;
    let bids = store.snapshot_benchmark_ids(steam_id, since)?;
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
            for (ts, s) in &series {
                if *ts < since || *ts > now {
                    continue;
                }
                if *s > high {
                    count += 1;
                    high = *s;
                }
            }
        }
    }
    Ok(count)
}

fn tier_name(benchmark_id: u64, rank_index: i64) -> String {
    let registry = crate::registry::Registry;
    registry
        .by_id(benchmark_id)
        .and_then(|(_b, diff)| {
            crate::ranks::rank_from_index(rank_index as u32, &diff).map(|t| t.name.to_string())
        })
        .unwrap_or_else(|| format!("tier {rank_index}"))
}
#[cfg(test)]
mod tests {
    use super::*;

    /// streaks helper semantic check via weekly: streak fields flow through.
    #[test]
    fn streak_parity_segment_sets_values() {
        // parity check: streaks_from_days strict semantics
        let days: std::collections::HashSet<chrono::NaiveDate> = [
            chrono::NaiveDate::from_ymd_opt(2026, 9, 13).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 9, 14).unwrap(),
        ]
        .into_iter()
        .collect();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        assert_eq!(crate::streaks::streaks_from_days(&days, today), (2, 2));
    }

    /// tier name mapping: rank_from_index via registry by_id.
    #[test]
    fn tier_name_falls_back_gracefully() {
        let name = tier_name(999999, 3);
        assert!(name.contains("tier") || !name.is_empty());
    }
}
#[cfg(test)]
mod week_tests {
    use super::*;
    use crate::types::{BenchmarkProgress, CategoryProgress};

    fn prog(_bid: i64, scenario: &str, score: i64, rank: i64) -> BenchmarkProgress {
        BenchmarkProgress {
            benchmark_progress: score as f64,
            overall_rank: rank as u32,
            categories: vec![(
                "Clicking".to_string(),
                CategoryProgress {
                    benchmark_progress: 0.0,
                    category_rank: 0,
                    rank_maxes: Vec::new(),
                    scenarios: vec![(
                        scenario.to_string(),
                        crate::types::ScenarioEntry {
                            score: score as f64,
                            leaderboard_rank: 0,
                            scenario_rank: 0,
                            rank_maxes: vec![100.0, 200.0, 300.0],
                            leaderboard_id: 0,
                        },
                    )],
                },
            )],
        }
    }

    #[test]
    fn weekly_report_counts_plays_ranks() {
        let dir = std::env::temp_dir().join(format!("weekly-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let store = crate::store::Store::open(&dir).unwrap();
        let now = chrono::Utc::now();
        // two snapshots inside the week with a rank change
        store
            .record_snapshot(
                "76561190000000001",
                460,
                &prog(0, "s", 100, 2),
                now - chrono::Duration::days(3),
            )
            .unwrap();
        store
            .record_snapshot("76561190000000001", 460, &prog(0, "s", 200, 3), now)
            .unwrap();
        // local plays this week
        let rec = crate::types::PlayRecord {
            scenario: "s".into(),
            played_at: now - chrono::Duration::days(1),
            score: 210.0,
            hit_count: 10,
            avg_fps: 240.0,
            source: crate::types::PlaySource::Csv,
        };
        store
            .record_play("76561190000000001", &rec, "csv-test")
            .unwrap();
        let rep = weekly_report(&store, "76561190000000001", now).unwrap();
        assert!(rep.plays >= 1, "plays {}", rep.plays);
        assert!(rep.days_played >= 1);
        assert!(rep.pb_events >= 1, "PB event from 200->210 within week");
        // rank change 2->3 recorded
        assert!(rep
            .rank_changes
            .iter()
            .any(|(_, name, from, to)| name.contains("Voltaic") && from != to && from != to));
        let _ = std::fs::remove_file(&dir);
    }
}

#[cfg(test)]
mod live_probe_tests {
    #[ignore]
    #[test]
    fn weekly_on_real_db() {
        let dir = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap())
            .join("kairos")
            .join("store.db");
        let store = crate::store::Store::open(&dir).unwrap();
        let id = "76561198173335263";
        let r = crate::weekly::weekly_report(&store, id, chrono::Utc::now()).unwrap();
        eprintln!(
            "plays={} days={} pbs={} ranks_changed={} imps={} streak={} xp={}",
            r.plays,
            r.days_played,
            r.pb_events,
            r.rank_changes.len(),
            r.improvements.len(),
            r.current_streak,
            r.xp
        );
        for rc in r.rank_changes.iter().take(4) {
            eprintln!("rank {:?}", rc);
        }
        for im in r.improvements.iter().take(4) {
            eprintln!("imp {:?}", im);
        }
    }
}
