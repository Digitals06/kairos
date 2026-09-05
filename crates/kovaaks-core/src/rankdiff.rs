//! Rank-diff: engine-computed overall-rank changes between consecutive
//! snapshots, used for the post-sync toast.
//!
//! Everything here is a pure read over [`Store`] + [`Registry`] — no network,
//! no Tauri — so the logic is unit-testable offline.

use serde::Serialize;

use crate::rankcalc::compute_rank;
use crate::store::StoredSnapshot;

/// One benchmark whose engine-computed overall rank changed between two
/// consecutive snapshots (surfaced as a post-sync toast).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankChange {
    pub benchmark_id: i64,
    pub benchmark_name: String,
    /// Engine ladder position, 1-based (`ladder_len + 1` = Complete).
    pub prev_rank: u32,
    pub cur_rank: u32,
    /// Display names ("" when unranked before).
    pub prev_name: String,
    pub cur_name: String,
    /// Ladder position moved up.
    pub improved: bool,
}

/// Engine-computed overall rank for one stored snapshot. Mirrors the GUI
/// card path: recompute via [`compute_rank`] on the snapshot's progress
/// (display units end-to-end), never trust the API's stored `overall_rank`
/// column — it is wrong for most benchmarks. Returns `None` when the
/// benchmark left the registry.
pub fn engine_rank_for_snapshot(
    registry: &crate::Registry,
    snap: &StoredSnapshot,
) -> Option<(u32, String)> {
    let (bench, difficulty) = registry.by_id(snap.benchmark_id as u64)?;
    let api_progress = stored_to_progress(snap);
    let result = compute_rank(&api_progress, bench, &difficulty);
    Some((result.rank, result.name))
}

/// Diff engine-computed overall ranks between the two newest snapshots of
/// every benchmark the player has snapshots for. The caller is responsible
/// for having just synced.
pub fn compute_rank_changes(
    store: &crate::Store,
    registry: &crate::Registry,
    steam_id: &str,
) -> crate::Result<Vec<RankChange>> {
    let mut changes = Vec::new();
    for benchmark_id in store.played_benchmarks(steam_id)? {
        let history = store.history(steam_id, benchmark_id)?;
        if history.len() < 2 {
            continue; // nothing to diff against
        }
        let prev = &history[history.len() - 2];
        let cur = history.last().expect("len >= 2");
        let (Some((prev_rank, prev_name)), Some((cur_rank, cur_name))) = (
            engine_rank_for_snapshot(registry, prev),
            engine_rank_for_snapshot(registry, cur),
        ) else {
            continue; // benchmark not engine-computable — skip silently
        };
        if prev_rank != cur_rank {
            let benchmark_name = registry
                .by_id(benchmark_id as u64)
                .map(|(b, _)| b.name.clone())
                .unwrap_or_else(|| format!("#{benchmark_id}"));
            changes.push(RankChange {
                benchmark_id,
                benchmark_name,
                prev_rank,
                cur_rank,
                prev_name,
                cur_name,
                improved: cur_rank > prev_rank,
            });
        }
    }
    Ok(changes)
}

/// Rebuild an API-shaped [`BenchmarkProgress`] from a stored snapshot so
/// the rank engine can consume it. Stored scores and rank_maxes are display
/// units; the engine works in display units too — no scaling. Scenario order
/// follows the stored `api_order` (category × 10 000 + index), matching
/// evxl's ordering. (Port of the GUI's `stored_to_progress` — the single
/// shared implementation; the GUI adapter calls this.)
pub fn stored_to_progress(snap: &StoredSnapshot) -> crate::types::BenchmarkProgress {
    use crate::types::{BenchmarkProgress, CategoryProgress, ScenarioEntry};
    let mut categories: Vec<(String, CategoryProgress)> = Vec::new();
    // Stored rows are ordered by (category, order_idx); rebuild the API's
    // per-category scenario maps in that same document order.
    for row in &snap.scenarios {
        let entry = ScenarioEntry {
            score: row.score as f64,
            leaderboard_rank: row.leaderboard_rank.max(0) as u64,
            scenario_rank: row.scenario_rank.max(0) as u32,
            rank_maxes: row.rank_maxes.clone(),
            leaderboard_id: 0,
        };
        match categories.last_mut() {
            Some((name, cat)) if *name == row.category => {
                cat.scenarios.push((row.scenario.clone(), entry));
            }
            _ => categories.push((
                row.category.clone(),
                CategoryProgress {
                    benchmark_progress: 0.0,
                    category_rank: row.category_rank.max(0) as u32,
                    rank_maxes: Vec::new(),
                    scenarios: vec![(row.scenario.clone(), entry)],
                },
            )),
        }
    }
    BenchmarkProgress {
        benchmark_progress: snap.benchmark_progress as f64,
        overall_rank: snap.overall_rank.max(0) as u32,
        categories,
    }
}
