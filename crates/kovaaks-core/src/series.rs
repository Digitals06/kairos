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

/// Merge local plays with new-high-filtered snapshot points into one
/// chronological series — THE dedup rule (same rounded integer = same run).
/// Public so per-call sites can reuse it on already-loaded parts.
pub fn merge_parts(plays: &[(DateTime<Utc>, f64)], snapshots: &[(DateTime<Utc>, f64)]) -> Series {
    let mut merged: Series = plays.to_vec();
    let is = crate::metrics::improving_only(snapshots);
    let play_scores: std::collections::HashSet<i64> =
        plays.iter().map(|(_, s)| s.round() as i64).collect();
    for (at, score) in is {
        if !play_scores.contains(&(score.round() as i64)) {
            merged.push((at, score));
        }
    }
    merged.sort_by_key(|(t, _)| *t);
    merged
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
}
