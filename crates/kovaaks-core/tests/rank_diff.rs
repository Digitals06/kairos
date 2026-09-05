//! Integration tests for the rank-diff module behind the post-sync toast.
//!
//! Offline: synthetic snapshots in a throwaway temp DB + the embedded
//! registry. No network. The engine path under test is the same
//! `compute_rank` call the GUI card uses — never the API's stored
//! `overall_rank` column (wrong for most benchmarks).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{TimeZone, Utc};
use kovaaks_core::types::{BenchmarkProgress, CategoryProgress, ScenarioEntry};
use kovaaks_core::{rankdiff, Registry, Store};

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_db(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .subsec_nanos();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "kovaaks-rankdiff-{tag}-{}-{nanos}-{seq}.db",
        std::process::id()
    ))
}

fn cleanup_db(path: &Path) {
    let base = path.to_string_lossy().into_owned();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{base}{suffix}"));
    }
}

const SID: &str = "76561190000000001";
/// Voltaic S3 / Advanced — `basic` rank method (floor over scenario ranks),
/// benchmark id 266 in the embedded registry.
const BENCH: i64 = 266;

/// Build a progress payload whose scenario ranks floor to `floor_rank`:
/// every scenario of the first category sits on `floor_rank` (1-based tier
/// index into the difficulty's rank_colors). `basic` ranks an overall
/// benchmark by the LOWEST achieved scenario rank (evxl `re`), so the
/// engine result must equal `floor_rank`.
fn progress_at_floor(registry: &Registry, floor_rank: u32) -> BenchmarkProgress {
    let (bench, difficulty) = registry.by_id(BENCH as u64).expect("VT S3 in registry");
    let _ = bench;
    // Walk subcategories (categoryName/subcategories/scenarioCount shape).
    // `basic` ranks by the best per-subcat rank via rank_of(score, rank_maxes),
    // so the achieved rank is driven by score vs rank_maxes below.
    let mut spans: Vec<(String, usize)> = Vec::new();
    for cat in &difficulty.categories {
        let Some(obj) = cat.as_object() else { continue };
        let cat_name = obj
            .get("categoryName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(subs) = obj.get("subcategories").and_then(|s| s.as_array()) {
            for sub in subs {
                let count = sub
                    .get("scenarioCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                if count > 0 {
                    spans.push((cat_name.clone(), count));
                }
            }
        }
    }
    assert!(!spans.is_empty(), "difficulty has subcategories");

    // Synthetic per-scenario ladder: 5 tiers, 100..500. A score of
    // `floor_rank * 100` sits exactly ON tier `floor_rank`'s threshold, so
    // rank_of interpolates to precisely `floor_rank` for every scenario.
    let mut entries: Vec<(String, ScenarioEntry)> = Vec::new();
    for (cat_name, count) in &spans {
        for _i in 0..*count {
            // Global running index: names must be unique per snapshot
            // (scenario_scores UNIQUE(snapshot_id, scenario, category)).
            entries.push((
                format!("{cat_name}#S{}", entries.len() + 1),
                ScenarioEntry {
                    score: floor_rank as f64 * 100.0,
                    leaderboard_rank: 1,
                    scenario_rank: floor_rank,
                    rank_maxes: vec![100.0, 200.0, 300.0, 400.0, 500.0],
                    leaderboard_id: 0,
                },
            ));
        }
    }

    let cat_name = spans[0].0.clone();

    BenchmarkProgress {
        benchmark_progress: floor_rank as f64 * 1000.0,
        overall_rank: 0,
        categories: vec![(
            cat_name,
            CategoryProgress {
                benchmark_progress: floor_rank as f64 * 1000.0,
                category_rank: floor_rank,
                rank_maxes: vec![1000.0, 2000.0, 3000.0, 4000.0, 5000.0],
                scenarios: entries,
            },
        )],
    }
}

/// Insert a snapshot with the given engine-floor rank, marking the
/// benchmark as played (played_benchmarks drives the diff loop).
fn snapshot_at(store: &Store, registry: &Registry, floor_rank: u32, when: chrono::DateTime<Utc>) {
    store
        .upsert_played(SID, BENCH, true, when)
        .expect("upsert played");
    let write = store
        .record_snapshot(SID, BENCH, &progress_at_floor(registry, floor_rank), when)
        .expect("record snapshot");
    let _ = write;
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[test]
fn rank_improvement_between_snapshots_is_detected() {
    let path = temp_db("improve");
    let store = Store::open(&path).expect("open store");
    let registry = Registry;

    let t0 = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    let t1 = Utc.timestamp_opt(1_700_000_600, 0).single().unwrap();

    snapshot_at(&store, &registry, 3, t0);
    snapshot_at(&store, &registry, 5, t1);

    let changes = rankdiff::compute_rank_changes(&store, &registry, SID).expect("diff");
    assert_eq!(changes.len(), 1, "one benchmark changed: {changes:?}");
    let c = &changes[0];
    assert_eq!(c.benchmark_id, BENCH);
    assert!(c.improved, "3 -> 5 is an improvement");
    assert_eq!(c.prev_rank, 3);
    assert_eq!(c.cur_rank, 5);

    cleanup_db(&path);
}

#[test]
fn rank_regression_is_flagged_not_improved() {
    let path = temp_db("regress");
    let store = Store::open(&path).expect("open store");
    let registry = Registry;

    let t0 = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    let t1 = Utc.timestamp_opt(1_700_000_600, 0).single().unwrap();

    snapshot_at(&store, &registry, 5, t0);
    snapshot_at(&store, &registry, 2, t1);

    let changes = rankdiff::compute_rank_changes(&store, &registry, SID).expect("diff");
    assert_eq!(changes.len(), 1);
    assert!(!changes[0].improved, "5 -> 2 is a regression");
    assert_eq!(changes[0].cur_rank, 2);

    cleanup_db(&path);
}

#[test]
fn single_snapshot_yields_no_changes() {
    let path = temp_db("single");
    let store = Store::open(&path).expect("open store");
    let registry = Registry;

    let t0 = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    snapshot_at(&store, &registry, 4, t0);

    let changes = rankdiff::compute_rank_changes(&store, &registry, SID).expect("diff");
    assert!(changes.is_empty(), "nothing to diff against");

    cleanup_db(&path);
}

#[test]
fn equal_ranks_yield_no_changes() {
    let path = temp_db("equal");
    let store = Store::open(&path).expect("open store");
    let registry = Registry;

    let t0 = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    let t1 = Utc.timestamp_opt(1_700_000_600, 0).single().unwrap();

    snapshot_at(&store, &registry, 4, t0);
    snapshot_at(&store, &registry, 4, t1);

    let changes = rankdiff::compute_rank_changes(&store, &registry, SID).expect("diff");
    assert!(changes.is_empty(), "same floor rank = no change");

    cleanup_db(&path);
}
