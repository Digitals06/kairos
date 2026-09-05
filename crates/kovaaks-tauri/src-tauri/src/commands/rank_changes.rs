use kovaaks_core::rankcalc::compute_rank;
use kovaaks_core::store::StoredSnapshot;
use kovaaks_core::types::BenchmarkProgress;

use super::{build_card, stored_to_progress, AppState};

/// One benchmark whose engine-computed overall rank changed between two
/// consecutive snapshots.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankChange {
    pub benchmark_id: i64,
    pub benchmark_name: String,
    /// Engine-computed ladder position, 1-based (`ladder_len + 1` = Complete).
    pub prev_rank: u32,
    pub cur_rank: u32,
    /// Display name of the previous rank ("" when the benchmark was unranked
    /// before, i.e. no engine rank at the older snapshot).
    pub prev_name: String,
    pub cur_name: String,
    /// Ladder position moved up (rank improved).
    pub improved: bool,
}

/// Engine-computed overall rank for one stored snapshot.
///
/// Mirrors `build_card`: recompute via `compute_rank` on the stored
/// snapshot's progress (display units end-to-end), never trust the API's
/// stored `overall_rank` column — we proved it wrong for most benchmarks.
/// Returns `None` when the benchmark left the registry or the engine has no
/// method for it.
fn engine_rank_for_snapshot(
    state: &AppState,
    snap: &StoredSnapshot,
) -> Option<(u32, String)> {
    let (bench, difficulty) = state.registry.by_id(snap.benchmark_id as u64)?;
    let api_progress: BenchmarkProgress = stored_to_progress(snap);
    let result = compute_rank(&api_progress, bench, &difficulty);
    Some((result.rank, result.name))
}

/// Diff engine-computed overall ranks between the two newest snapshots of
/// every benchmark the player has snapshots for.
///
/// Only reads local state — the caller is responsible for having just synced.
pub fn rank_changes(state: &AppState, steam_id: &str) -> kovaaks_core::Result<Vec<RankChange>> {
    let mut changes = Vec::new();
    for benchmark_id in state.store.played_benchmarks(steam_id)? {
        let history = state.store.history(steam_id, benchmark_id)?;
        if history.len() < 2 {
            continue; // nothing to diff against
        }
        let prev = &history[history.len() - 2];
        let cur = history.last().expect("len >= 2");
        let (Some((prev_rank, prev_name)), Some((cur_rank, cur_name))) = (
            engine_rank_for_snapshot(state, prev),
            engine_rank_for_snapshot(state, cur),
        ) else {
            continue; // benchmark not engine-computable — skip silently
        };
        if prev_rank != cur_rank {
            changes.push(RankChange {
                benchmark_id,
                benchmark_name: bench_name(state, benchmark_id),
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

fn bench_name(state: &AppState, benchmark_id: i64) -> String {
    state
        .registry
        .by_id(benchmark_id as u64)
        .map(|(b, _)| b.name.clone())
        .unwrap_or_else(|| format!("#{}", benchmark_id))
}
