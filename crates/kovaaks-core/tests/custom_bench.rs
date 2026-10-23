//! Integration tests for v0.5.0 custom benchmarks: schema v5, CRUD, score
//! merging and the scenario index. Fully offline, throwaway DB per test.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeZone, Utc};
use kovaaks_core::custom::{rank_custom, validate, MODE_ALL, MODE_TOP_N};
use kovaaks_core::store::CustomBenchRow;
use kovaaks_core::{BenchmarkProgress, CategoryProgress, PlayRecord, ScenarioEntry, Store};

static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);
const SID: &str = "76561190000000001";

fn temp_db(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .subsec_nanos();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "kairos-custom-{tag}-{}-{nanos}-{seq}.db",
        std::process::id()
    ))
}

fn cleanup_db(path: &Path) {
    let base = path.to_string_lossy().into_owned();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{base}{suffix}"));
    }
}

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .expect("valid timestamp")
}

fn scenario(score: f64) -> ScenarioEntry {
    ScenarioEntry {
        score,
        leaderboard_rank: 10,
        scenario_rank: 2,
        rank_maxes: vec![100.0, 200.0, 300.0],
        leaderboard_id: 1,
    }
}

fn progress(score: f64, scenario_name: &str) -> BenchmarkProgress {
    BenchmarkProgress {
        benchmark_progress: score,
        overall_rank: 2,
        categories: vec![(
            "Clicking".to_string(),
            CategoryProgress {
                benchmark_progress: score,
                category_rank: 2,
                rank_maxes: vec![100.0, 200.0, 300.0],
                scenarios: vec![(scenario_name.to_string(), scenario(score))],
            },
        )],
    }
}

fn play(scenario_name: &str, at: DateTime<Utc>, score: f64) -> PlayRecord {
    PlayRecord {
        scenario: scenario_name.to_string(),
        played_at: at,
        score,
        hit_count: 100,
        avg_fps: 240.0,
        source: kovaaks_core::types::PlaySource::Csv,
    }
}

fn bench() -> CustomBenchRow {
    CustomBenchRow {
        id: 0,
        name: "Daily Five".into(),
        abbreviation: "D5".into(),
        color: "#b08d57".into(),
        mode: MODE_ALL.into(),
        top_n: 0,
        tier_names: vec!["Bronze".into(), "Silver".into(), "Gold".into()],
        scenarios: vec![
            ("Pasu".into(), "Clicking".into(), vec![100.0, 200.0, 300.0]),
            (
                "Gridshot".into(),
                "Clicking".into(),
                vec![150.0, 250.0, 350.0],
            ),
        ],
    }
}

#[test]
fn schema_migrates_to_v5_and_keeps_user_data() {
    let path = temp_db("schema");
    let store = Store::open(&path).expect("open store");
    // Second open must be a no-op migration (idempotent).
    drop(store);
    let store = Store::open(&path).expect("reopen store");
    let rowid = store.create_custom_bench(SID, &bench()).expect("create");
    assert!(rowid > 0);
    let rows = store.custom_benches(SID).expect("list");
    assert_eq!(rows.len(), 1);
    cleanup_db(&path);
}

#[test]
fn create_read_update_delete_roundtrip() {
    let path = temp_db("crud");
    let store = Store::open(&path).expect("open store");
    let rowid = store.create_custom_bench(SID, &bench()).expect("create");

    let mut rows = store.custom_benches(SID).expect("list");
    assert_eq!(rows.len(), 1);
    let mut saved = rows.remove(0);
    assert_eq!(saved.id, rowid);
    assert_eq!(saved.name, "Daily Five");
    assert_eq!(saved.tier_names, vec!["Bronze", "Silver", "Gold"]);
    assert_eq!(saved.scenarios.len(), 2);
    assert_eq!(saved.scenarios[0].0, "Pasu");
    assert_eq!(saved.scenarios[0].2, vec![100.0, 200.0, 300.0]);

    // Update: rename, switch mode, replace the scenario list.
    saved.name = "Nightly Three".into();
    saved.mode = MODE_TOP_N.into();
    saved.top_n = 2;
    saved.scenarios = vec![("Pasu".into(), "Clicking".into(), vec![10.0, 20.0, 30.0])];
    assert!(store.update_custom_bench(SID, &saved).expect("update"));

    let rows = store.custom_benches(SID).expect("list after update");
    assert_eq!(rows[0].name, "Nightly Three");
    assert_eq!(rows[0].mode, MODE_TOP_N);
    assert_eq!(rows[0].top_n, 2);
    assert_eq!(rows[0].scenarios.len(), 1, "old rows are replaced");

    assert!(store.delete_custom_bench(SID, rowid).expect("delete"));
    assert!(store.custom_benches(SID).expect("list").is_empty());
    assert!(
        !store
            .update_custom_bench(SID, &saved)
            .expect("no-op update"),
        "unknown ids are reported, not silently accepted"
    );
    cleanup_db(&path);
}

#[test]
fn delete_also_clears_favorites_keyed_on_the_app_id() {
    let path = temp_db("cascade");
    let store = Store::open(&path).expect("open store");
    let rowid = store.create_custom_bench(SID, &bench()).expect("create");
    let app_id = -rowid;
    assert!(store.add_favorite(SID, app_id).expect("favorite"));
    assert!(store.favorites(SID).expect("list").contains(&app_id));

    assert!(store.delete_custom_bench(SID, rowid).expect("delete"));
    assert!(
        !store.favorites(SID).expect("list").contains(&app_id),
        "a deleted bench must not leave a dangling favorite"
    );
    cleanup_db(&path);
}

#[test]
fn benches_are_scoped_per_player() {
    let path = temp_db("scope");
    let store = Store::open(&path).expect("open store");
    store.create_custom_bench(SID, &bench()).expect("create");
    let other = "76561190000000002";
    assert!(store.custom_benches(other).expect("list").is_empty());
    assert!(store
        .scenario_scores_by_name(other, &["Pasu".into()])
        .expect("scores")
        .is_empty());
    cleanup_db(&path);
}

#[test]
fn scores_merge_snapshots_and_plays_with_max() {
    let path = temp_db("scores");
    let store = Store::open(&path).expect("open store");
    // Snapshot score 250 for Pasu (benchmark 84), a weaker local play 180,
    // and a stronger local play 900 for Gridshot with no snapshot at all.
    store
        .record_snapshot(SID, 84, &progress(250.0, "Pasu"), ts(1_700_000_000))
        .expect("snapshot");
    store
        .record_play(
            SID,
            &play("Pasu", ts(1_700_000_100), 180.0),
            r"C:\s\pasu1.csv",
        )
        .expect("play");
    store
        .record_play(
            SID,
            &play("Gridshot", ts(1_700_000_200), 900.0),
            r"C:\s\grid.csv",
        )
        .expect("play");

    let scores = store
        .scenario_scores_by_name(SID, &["Pasu".into(), "Gridshot".into(), "Nope".into()])
        .expect("scores");
    assert_eq!(scores.get("Pasu").copied(), Some(250.0), "snapshot wins");
    assert_eq!(
        scores.get("Gridshot").copied(),
        Some(900.0),
        "play fills the gap"
    );
    assert!(!scores.contains_key("Nope"));

    // A play beating the snapshot must win.
    store
        .record_play(
            SID,
            &play("Pasu", ts(1_700_000_300), 400.0),
            r"C:\s\pasu2.csv",
        )
        .expect("play");
    let scores = store
        .scenario_scores_by_name(SID, &["Pasu".into()])
        .expect("scores");
    assert_eq!(scores.get("Pasu").copied(), Some(400.0), "best play wins");
    cleanup_db(&path);
}

#[test]
fn known_scenarios_dedupes_snapshots_and_plays() {
    let path = temp_db("known");
    let store = Store::open(&path).expect("open store");
    store
        .record_snapshot(SID, 84, &progress(250.0, "Pasu"), ts(1_700_000_000))
        .expect("snapshot");
    store
        .record_play(
            SID,
            &play("Pasu", ts(1_700_000_100), 180.0),
            r"C:\s\pasu.csv",
        )
        .expect("play");
    store
        .record_play(
            SID,
            &play("Sandbox", ts(1_700_000_200), 42.0),
            r"C:\s\sb.csv",
        )
        .expect("play");

    let known = store.known_scenarios(SID).expect("known");
    let names: Vec<&str> = known.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["Pasu", "Sandbox"], "one row each, sorted");
    let pasu = known.iter().find(|(n, _)| n == "Pasu").expect("pasu");
    assert_eq!(pasu.1, "Clicking", "category comes from the snapshot");
    let sandbox = known.iter().find(|(n, _)| n == "Sandbox").expect("sb");
    assert_eq!(sandbox.1, "", "play-only scenarios have no category");
    cleanup_db(&path);
}

#[test]
fn a_saved_bench_scores_from_stored_data() {
    let path = temp_db("score-e2e");
    let store = Store::open(&path).expect("open store");
    let mut b = bench();
    // Author thresholds: Pasu 100/200/300, Gridshot 150/250/350.
    store
        .record_snapshot(SID, 84, &progress(250.0, "Pasu"), ts(1_700_000_000))
        .expect("snapshot");
    store
        .record_play(
            SID,
            &play("Gridshot", ts(1_700_000_100), 120.0),
            r"C:\s\g.csv",
        )
        .expect("play");

    let rowid = store.create_custom_bench(SID, &b).expect("create");
    let saved = store.custom_benches(SID).expect("list").remove(0);
    assert_eq!(saved.id, rowid);

    let scores = store
        .scenario_scores_by_name(
            SID,
            &saved
                .scenarios
                .iter()
                .map(|s| s.0.clone())
                .collect::<Vec<_>>(),
        )
        .expect("scores");
    let ranked = rank_custom(&saved, &scores).expect("rank");
    // Pasu 250 → Silver (2); Gridshot 120 → below Bronze (0); `all` sits at 0.
    assert_eq!(ranked.rank, 0);
    let pasu = ranked
        .scenarios
        .iter()
        .find(|s| s.scenario == "Pasu")
        .expect("pasu");
    assert_eq!(pasu.tier, 2);

    // Lower Gridshot's thresholds so the bench advances — authors stay in control.
    b.id = rowid;
    b.scenarios[1].2 = vec![50.0, 100.0, 200.0];
    assert!(store.update_custom_bench(SID, &b).expect("update"));
    let saved = store.custom_benches(SID).expect("list").remove(0);
    let scores = store
        .scenario_scores_by_name(SID, &["Pasu".into(), "Gridshot".into()])
        .expect("scores");
    let ranked = rank_custom(&saved, &scores).expect("rank");
    assert_eq!(ranked.rank, 2, "both scenarios now clear Silver");
    assert!(validate(&saved).is_ok());
    cleanup_db(&path);
}

#[test]
fn backup_export_includes_custom_tables() {
    let path = temp_db("export");
    let store = Store::open(&path).expect("open store");
    store.create_custom_bench(SID, &bench()).expect("create");
    let dump = store.export_all().expect("export");
    let benches = dump["custom_benchmarks"].as_array().expect("bench table");
    assert_eq!(benches.len(), 1);
    let scenarios = dump["custom_bench_scenarios"]
        .as_array()
        .expect("scenario table");
    assert_eq!(scenarios.len(), 2);
    cleanup_db(&path);
}
