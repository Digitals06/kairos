//! Score-series module: the single owner of "merged score series".
//!
//! A **series** is one scenario's chronological run history for one player:
//! local CSV plays merged with the new-high snapshot points, snapshot points
//! that echo an already-recorded local play dropped (the same run observed by
//! two sources must not double-count). The merge/dedup semantics used to
//! exist in three shapes across metrics.rs and weekly.rs; they live only
//! here now. Callers: charts/detail, weekly recap (fused bulk pass), the
//! XP streak model, and sync staleness checks.

use crate::store::Store;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// One scenario's merged chronological series.
pub type Series = Vec<(DateTime<Utc>, f64)>;

/// A key identifying one scenario within one benchmark snapshot stream.
pub type ScenarioKey = (i64, String);

/// Pre-partitioned bulk inputs for a series-map load: plays by scenario,
/// snapshots by benchmark (each snapshot keeping only (ts, scenario, score)).
pub struct SeriesInputs {
    pub plays: HashMap<String, Vec<(DateTime<Utc>, f64)>>,
    /// benchmark_id -> [(captured_at, scenario, score)] from ALL its snapshots.
    pub snapshots: HashMap<i64, Vec<(DateTime<Utc>, String, f64)>>,
}

/// Keep only the snapshots that set a new high for a scenario, in order.
///
/// A sync snapshot that doesn't improve the score almost always means the
/// scenario wasn't replayed since the previous sync — recording it would
/// drag averages toward stale data and spray flat dots across the chart.
/// When the scenario WAS replayed without a new high, the local CSV plays
/// already describe the average, so the snapshot adds nothing.
///
/// The running max seeds at `0.0`: an all-zero (never-played) series
/// contributes nothing. Input must be chronological (as `Store::history`
/// returns); output preserves order.
pub fn improving_only(series: &[(DateTime<Utc>, f64)]) -> Vec<(DateTime<Utc>, f64)> {
    let mut best = 0.0f64;
    let mut kept = Vec::new();
    for &(at, score) in series {
        if score > best && score.is_finite() {
            best = score;
            kept.push((at, score));
        }
    }
    kept
}

/// Two scores are the same run when they round to the same integer. KovaaK's
/// scenario scores are precise floats locally, but the sync API echoes them
/// ROUNDED TO INTEGERS (play 1558.668 syncs as 1559, 805.628 as 806) — so a
/// raw-epsilon comparison never matches the echo. Rounding both sides first
/// makes the rule exact: the same float always rounds to the same integer,
/// and a genuinely different run (e.g. 1125.70 vs 1125.29) rounds apart.
pub fn is_same_run(a: f64, b: f64) -> bool {
    (a.round() - b.round()).abs() < 0.5
}

/// Merge local plays with (already new-high-filtered) snapshot points into one
/// chronological series, dropping snapshot points that duplicate a local play.
///
/// A snapshot that is the same run as a local play (see [`is_same_run`]) is
/// the same scenario run observed twice — keeping both would add a phantom
/// sample at the sync timestamp and truncate the real average improvement, so
/// the snapshot point is skipped and the play (exact time) is kept. Plays are
/// always kept verbatim. The `bool` flag is `true` for a play, `false` for a
/// snapshot.
pub fn merge_plays_snapshots_dedup(
    plays: &[(String, DateTime<Utc>, f64)],
    snapshots: &[(DateTime<Utc>, f64)],
) -> Vec<(DateTime<Utc>, f64, bool)> {
    // O(n+m): quantized play scores in a set (`is_same_run` = same rounded
    // integer), so each snapshot point tests one hash lookup instead of a
    // linear scan over every play of the scenario.
    let play_scores: std::collections::HashSet<i64> =
        plays.iter().map(|(_, _, s)| s.round() as i64).collect();
    let mut merged: Vec<(DateTime<Utc>, f64, bool)> = plays
        .iter()
        .map(|(_, at, score)| (*at, *score, true))
        .collect();
    for &(at, score) in snapshots {
        if !play_scores.contains(&(score.round() as i64)) {
            merged.push((at, score, false));
        }
    }
    merged.sort_by_key(|(t, _, _)| *t);
    merged
}

/// Merge local plays with new-high-filtered snapshot points into one
/// chronological series — THE dedup rule (same rounded integer = same run).
/// Public so per-call sites can reuse it on already-loaded parts. Thin
/// projection over [`merge_plays_snapshots_dedup`]: pairs in, pairs out.
pub fn merge_parts(plays: &[(DateTime<Utc>, f64)], snapshots: &[(DateTime<Utc>, f64)]) -> Series {
    let flagged_plays: Vec<(String, DateTime<Utc>, f64)> = plays
        .iter()
        .map(|&(at, score)| (String::new(), at, score))
        .collect();
    merge_plays_snapshots_dedup(&flagged_plays, &improving_only(snapshots))
        .into_iter()
        .map(|(at, score, _)| (at, score))
        .collect()
}

/// One scenario's merged (plays + new-high snapshots) chronological series —
/// the single owner of the dedup semantics, reused by consistency analytics.
pub fn scenario_series(
    store: &Store,
    steam_id: &str,
    benchmark_id: i64,
    scenario: &str,
) -> crate::Result<Vec<(DateTime<Utc>, f64)>> {
    let plays: Vec<(String, DateTime<Utc>, f64)> = store
        .plays_history(steam_id, scenario)?
        .into_iter()
        .map(|rec| (rec.scenario.clone(), rec.played_at, rec.score))
        .collect();
    let snapshots: Vec<(DateTime<Utc>, f64)> = store
        .history(steam_id, benchmark_id)?
        .iter()
        .filter_map(|snap| {
            snap.scenarios
                .iter()
                .find(|r| r.scenario == scenario)
                .map(|row| (snap.captured_at, row.score as f64))
        })
        .collect();
    // New-high snapshots only; snapshot points duplicating a local play (the
    // same run echoed by the sync) are dropped — see merge_plays_snapshots_dedup.
    Ok(
        merge_plays_snapshots_dedup(&plays, &improving_only(&snapshots))
            .into_iter()
            .map(|(at, score, _)| (at, score))
            .collect(),
    )
}

/// Bulk series map: ONE history pass per benchmark, ONE plays query total.
/// `bids` drives which benchmarks; scenarios not in the snapshots produce no
/// entry (callers iterate the entries, not the expectation set).
pub fn series_map(
    store: &Store,
    steam_id: &str,
    bids: &[i64],
) -> crate::Result<HashMap<ScenarioKey, Series>> {
    let mut out: HashMap<ScenarioKey, Series> = HashMap::new();
    let mut by_scenario_plays: HashMap<String, Vec<(DateTime<Utc>, f64)>> = Default::default();
    for rec in store.all_plays(steam_id)? {
        by_scenario_plays
            .entry(rec.scenario.clone())
            .or_default()
            .push((rec.played_at, rec.score));
    }
    for &bid in bids {
        let history = store.history(steam_id, bid)?;
        if history.is_empty() {
            continue;
        }
        // collect per-scenario snapshot points from this bid's history once
        let mut snap_points: HashMap<&str, Vec<(DateTime<Utc>, f64)>> = Default::default();
        for snap in &history {
            for row in &snap.scenarios {
                snap_points
                    .entry(row.scenario.as_str())
                    .or_default()
                    .push((snap.captured_at, row.score as f64));
            }
        }
        for (scenario, points) in snap_points {
            let plays = by_scenario_plays
                .get(scenario)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let merged = merge_parts(plays, &points);
            out.insert((bid, scenario.to_string()), merged);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    #[test]
    fn merge_dedups_snapshot_echo_and_keeps_new_highs() {
        let plays = vec![(ts(2026, 9, 1), 1558.668), (ts(2026, 9, 2), 900.0)];
        let snaps = vec![
            // echo of the Sep-1 play (rounded 1559)
            (ts(2026, 9, 3), 1559.0),
            // genuine new high
            (ts(2026, 9, 4), 1700.0),
        ];
        let s = merge_parts(&plays, &snaps);
        assert_eq!(s.len(), 3, "echo dropped, high kept: {s:?}");
        assert_eq!(s[2].1, 1700.0);
    }

    #[test]
    fn merge_is_chronological() {
        let plays = vec![(ts(2026, 9, 5), 120.0)];
        let snaps = vec![(ts(2026, 9, 1), 100.0), (ts(2026, 9, 7), 150.0)];
        let s = merge_parts(&plays, &snaps);
        let times: Vec<_> = s.iter().map(|(t, _)| *t).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted);
    }

    #[test]
    fn improving_only_keeps_strict_highs_in_order() {
        let day = |d: u32| ts(2026, 1, d);
        let series = vec![
            (day(1), 0.0),   // unplayed: never a data point
            (day(2), 500.0), // baseline high: kept
            (day(3), 500.0), // stale repeat: dropped
            (day(4), 400.0), // below high: dropped
            (day(5), 450.0), // partial recovery, still stale: dropped
            (day(6), 600.0), // new high: kept
            (day(7), 600.0), // stale repeat: dropped
        ];
        let kept = improving_only(&series);
        assert_eq!(
            kept,
            vec![(day(2), 500.0), (day(6), 600.0)],
            "only new highs survive, in order"
        );
        assert!(improving_only(&[]).is_empty());
        assert_eq!(
            improving_only(&[(day(1), 300.0)]),
            vec![(day(1), 300.0)],
            "a lone baseline is informative"
        );
        assert!(
            improving_only(&[(day(1), 0.0), (day(2), 0.0)]).is_empty(),
            "all-zero series contributes nothing"
        );
    }
}
