//! Integration tests for the SQLite snapshot store (plan Task 1.5).
//!
//! Fully offline: every test opens its own throwaway SQLite database in the
//! OS temp dir (unique file name per call — std only, no new dependencies).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeZone, Utc};
use kovaaks_core::{
    BenchmarkProgress, CategoryProgress, PlayRecord, PlayerProfile, ScenarioEntry, SnapshotWrite,
    Store,
};
use rusqlite::params;

// ---------- temp-dir helpers (std only) ----------

static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

/// Unique temp DB path per call (temp_dir + tag + pid + nanos + counter).
fn temp_db(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .subsec_nanos();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "kovaaks-store-{tag}-{}-{nanos}-{seq}.db",
        std::process::id()
    ))
}

/// Best-effort cleanup of the DB file plus its WAL sidecars.
fn cleanup_db(path: &Path) {
    let base = path.to_string_lossy().into_owned();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{base}{suffix}"));
    }
}

// ---------- fixture builders ----------

const SID: &str = "76561190000000001";

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .expect("valid timestamp")
}

fn scenario(score: f64, leaderboard_rank: u64, scenario_rank: u32) -> ScenarioEntry {
    ScenarioEntry {
        score,
        leaderboard_rank,
        scenario_rank,
        rank_maxes: vec![40000.0, 80000.0, 120000.0, 160000.0],
        leaderboard_id: 98059,
    }
}

fn category(progress: f64, rank: u32, scenarios: Vec<(&str, ScenarioEntry)>) -> CategoryProgress {
    CategoryProgress {
        benchmark_progress: progress,
        category_rank: rank,
        rank_maxes: vec![40000.0, 80000.0, 120000.0, 160000.0],
        scenarios: scenarios
            .into_iter()
            .map(|(name, entry)| (name.to_string(), entry))
            .collect(),
    }
}

fn progress(
    overall: f64,
    rank: u32,
    categories: Vec<(&str, CategoryProgress)>,
) -> BenchmarkProgress {
    BenchmarkProgress {
        benchmark_progress: overall,
        overall_rank: rank,
        categories: categories
            .into_iter()
            .map(|(name, cat)| (name.to_string(), cat))
            .collect(),
    }
}

fn play(scenario_name: &str, at: DateTime<Utc>, score: f64, hits: u64, fps: f64) -> PlayRecord {
    PlayRecord {
        scenario: scenario_name.to_string(),
        played_at: at,
        score,
        hit_count: hits,
        avg_fps: fps,
        source: "csv".to_string(),
    }
}

fn profile(persona: &str, country: &str) -> PlayerProfile {
    PlayerProfile {
        steam_id: SID.to_string(),
        persona: persona.to_string(),
        avatar_url: "https://example.com/avatar.png".to_string(),
        country: country.to_string(),
    }
}

// ---------- schema ----------

#[test]
fn open_creates_schema_v1_and_reopen_preserves_data() {
    let path = temp_db("schema");
    {
        let store = Store::open(&path).expect("open must create + migrate");
        store.set_meta("profile", SID).expect("set_meta");
    }
    // Fresh raw connection inspects the persisted schema.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        version, 4,
        "migration must set user_version to current schema"
    );
    for table in [
        "players",
        "benchmarks_playing",
        "snapshots",
        "scenario_scores",
        "plays",
        "meta",
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "table {table} must exist");
    }
    // Reopen through Store: migration is a no-op and data survives.
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get_meta("profile").unwrap().as_deref(), Some(SID));
    cleanup_db(&path);
}

#[test]
fn schema_v1_matches_plan_constraints() {
    let path = temp_db("ddl");
    {
        Store::open(&path).unwrap();
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    let ddl = |name: &str| -> String {
        conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |r| r.get(0),
        )
        .unwrap()
    };
    let plays = ddl("plays");
    assert!(
        plays.contains("csv_path TEXT NOT NULL UNIQUE"),
        "plays.csv_path must be UNIQUE: {plays}"
    );
    assert!(
        plays.contains("UNIQUE (steam_id, csv_path)"),
        "plays must carry UNIQUE(steam_id, csv_path): {plays}"
    );
    let scores = ddl("scenario_scores");
    assert!(
        scores.contains("REFERENCES snapshots(id) ON DELETE CASCADE"),
        "scenario_scores.snapshot_id must cascade: {scores}"
    );
    let snapshots = ddl("snapshots");
    assert!(
        snapshots.contains("UNIQUE (steam_id, benchmark_id, captured_at)"),
        "snapshots must carry the plan's UNIQUE triple: {snapshots}"
    );
    let playing = ddl("benchmarks_playing");
    assert!(
        playing.contains("PRIMARY KEY (steam_id, benchmark_id)"),
        "benchmarks_playing must carry the composite PK: {playing}"
    );
    cleanup_db(&path);
}

// ---------- snapshots ----------

#[test]
fn record_snapshot_roundtrips_through_latest_and_history() {
    let path = temp_db("roundtrip");
    let store = Store::open(&path).unwrap();
    let p = progress(
        180000.0,
        4,
        vec![
            (
                "Tracking",
                category(
                    180000.0,
                    4,
                    vec![("VT Pasu Novice S5", scenario(128161.0, 102, 4))],
                ),
            ),
            (
                "Clicking",
                category(
                    60000.0,
                    3,
                    vec![("VT Noviceaj Gridshot S5", scenario(60000.0, 77, 3))],
                ),
            ),
        ],
    );
    let out = store
        .record_snapshot(SID, 459, &p, ts(1_700_000_000))
        .expect("record_snapshot");
    let id = match out {
        SnapshotWrite::Inserted { id } => id,
        other => panic!("expected Inserted, got {other:?}"),
    };
    let snap = store
        .latest(SID, 459)
        .unwrap()
        .expect("snapshot must exist after insert");
    assert_eq!(snap.id, id);
    assert_eq!(snap.captured_at, ts(1_700_000_000));
    assert_eq!(snap.benchmark_progress, 180000);
    assert_eq!(snap.overall_rank, 4);
    assert_eq!(snap.scenarios.len(), 2);
    let pasu = snap
        .scenarios
        .iter()
        .find(|s| s.scenario == "VT Pasu Novice S5")
        .expect("pasu row");
    assert_eq!(pasu.category, "Tracking");
    assert_eq!(pasu.score, 128161);
    assert_eq!(pasu.leaderboard_rank, 102);
    assert_eq!(pasu.scenario_rank, 4);
    assert_eq!(pasu.rank_maxes, vec![40000.0, 80000.0, 120000.0, 160000.0]);
    let history = store.history(SID, 459).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, id);
    // Absent benchmark -> None.
    assert!(store.latest(SID, 123).unwrap().is_none());
    cleanup_db(&path);
}

#[test]
fn record_snapshot_dedups_identical_progress_by_updating_captured_at() {
    let path = temp_db("dedup");
    let store = Store::open(&path).unwrap();
    let p = progress(
        180000.0,
        4,
        vec![(
            "Tracking",
            category(
                180000.0,
                4,
                vec![("VT Pasu Novice S5", scenario(128161.0, 102, 4))],
            ),
        )],
    );
    let first = store
        .record_snapshot(SID, 459, &p, ts(1_700_000_000))
        .unwrap();
    let second = store
        .record_snapshot(SID, 459, &p, ts(1_700_010_000))
        .unwrap();
    match (&first, &second) {
        (SnapshotWrite::Inserted { id }, SnapshotWrite::Deduplicated { id: id2 }) => {
            assert_eq!(id, id2, "dedup must keep the original snapshot row");
        }
        other => panic!("expected Inserted then Deduplicated, got {other:?}"),
    }
    let history = store.history(SID, 459).unwrap();
    assert_eq!(history.len(), 1, "dedup must not insert a second snapshot");
    assert_eq!(
        history[0].captured_at,
        ts(1_700_010_000),
        "captured_at must be bumped on dedup"
    );
    cleanup_db(&path);
}

#[test]
fn dedup_is_insensitive_to_scenario_map_iteration_order() {
    let path = temp_db("dedup-order");
    let store = Store::open(&path).unwrap();
    let make = |reverse: bool| {
        let pasu = ("VT Pasu Novice S5", scenario(128161.0, 102, 4));
        let psalm = ("VT psalmTS Novice S5", scenario(90000.0, 51, 3));
        let mut scen: Vec<(&str, ScenarioEntry)> = vec![pasu, psalm];
        if reverse {
            scen.reverse();
        }
        progress(180000.0, 4, vec![("Tracking", category(180000.0, 4, scen))])
    };
    let first = store
        .record_snapshot(SID, 459, &make(false), ts(1_700_000_000))
        .unwrap();
    let second = store
        .record_snapshot(SID, 459, &make(true), ts(1_700_000_500))
        .unwrap();
    assert!(matches!(first, SnapshotWrite::Inserted { .. }));
    // v3: scenario order is meaningful (API document order is preserved), so
    // a reordered payload is a genuinely different snapshot, not a duplicate.
    assert!(
        matches!(second, SnapshotWrite::Inserted { .. }),
        "reordered scenarios are a different payload (order is preserved now)"
    );
    assert_eq!(store.history(SID, 459).unwrap().len(), 2);

    // Identical to the FIRST payload, but dedup only compares the NEWEST
    // snapshot (the reversed one) — so this inserts too.
    let third = store
        .record_snapshot(SID, 459, &make(false), ts(1_700_001_000))
        .unwrap();
    assert!(
        matches!(third, SnapshotWrite::Inserted { .. }),
        "dedup compares only the newest snapshot, which differs in order"
    );
    assert_eq!(store.history(SID, 459).unwrap().len(), 3);

    // An exact repeat of the newest payload still dedups.
    let fourth = store
        .record_snapshot(SID, 459, &make(false), ts(1_700_001_500))
        .unwrap();
    assert!(
        matches!(fourth, SnapshotWrite::Deduplicated { .. }),
        "identical payloads must dedup"
    );
    assert_eq!(store.history(SID, 459).unwrap().len(), 3);
    cleanup_db(&path);
}

#[test]
fn changed_scores_create_a_new_snapshot() {
    let path = temp_db("dedup-changed");
    let store = Store::open(&path).unwrap();
    let old = progress(
        180000.0,
        4,
        vec![(
            "Tracking",
            category(
                180000.0,
                4,
                vec![("VT Pasu Novice S5", scenario(128161.0, 102, 4))],
            ),
        )],
    );
    let new = progress(
        180000.0,
        4,
        vec![(
            "Tracking",
            category(
                180000.0,
                4,
                vec![("VT Pasu Novice S5", scenario(130000.0, 101, 4))],
            ),
        )],
    );
    assert!(matches!(
        store
            .record_snapshot(SID, 459, &old, ts(1_700_000_000))
            .unwrap(),
        SnapshotWrite::Inserted { .. }
    ));
    assert!(matches!(
        store
            .record_snapshot(SID, 459, &new, ts(1_700_010_000))
            .unwrap(),
        SnapshotWrite::Inserted { .. }
    ));
    let history = store.history(SID, 459).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].captured_at, ts(1_700_000_000));
    assert_eq!(history[1].captured_at, ts(1_700_010_000));
    let latest = store.latest(SID, 459).unwrap().unwrap();
    assert_eq!(latest.scenarios[0].score, 130000);
    cleanup_db(&path);
}

#[test]
fn history_is_ordered_by_captured_at_regardless_of_insert_order() {
    let path = temp_db("history-order");
    let store = Store::open(&path).unwrap();
    let mk = |overall: f64| {
        progress(
            overall,
            1,
            vec![(
                "Tracking",
                category(
                    overall,
                    1,
                    vec![("VT Pasu Novice S5", scenario(overall, 1, 1))],
                ),
            )],
        )
    };
    // Insert out of chronological order on purpose.
    store
        .record_snapshot(SID, 459, &mk(300.0), ts(1_700_000_300))
        .unwrap();
    store
        .record_snapshot(SID, 459, &mk(100.0), ts(1_700_000_100))
        .unwrap();
    store
        .record_snapshot(SID, 459, &mk(200.0), ts(1_700_000_200))
        .unwrap();
    let history = store.history(SID, 459).unwrap();
    let times: Vec<DateTime<Utc>> = history.iter().map(|s| s.captured_at).collect();
    assert_eq!(
        times,
        vec![ts(1_700_000_100), ts(1_700_000_200), ts(1_700_000_300)],
        "history must ascend by captured_at"
    );
    let latest = store.latest(SID, 459).unwrap().unwrap();
    assert_eq!(latest.captured_at, ts(1_700_000_300));
    assert_eq!(latest.benchmark_progress, 300);
    cleanup_db(&path);
}

// ---------- plays ----------

#[test]
fn record_play_roundtrips_and_filters_by_scenario() {
    let path = temp_db("plays");
    let store = Store::open(&path).unwrap();
    let pasu = play(
        "VT Pasu Novice S5",
        ts(1_700_000_000),
        959.120239,
        145,
        239.5,
    );
    assert!(store
        .record_play(
            SID,
            &pasu,
            r"C:\stats\VT Pasu Novice S5 - Challenge - 2023.11.14-19.20.00 Stats.csv"
        )
        .unwrap());
    let grid = play(
        "VT Noviceaj Gridshot S5",
        ts(1_700_000_100),
        1200.0,
        200,
        240.0,
    );
    assert!(store
        .record_play(SID, &grid, r"C:\stats\gridshot.csv")
        .unwrap());
    let rows = store.plays_history(SID, "VT Pasu Novice S5").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0], pasu, "stored play must round-trip exactly");
    assert!(store.plays_history(SID, "Nope").unwrap().is_empty());
    cleanup_db(&path);
}

#[test]
fn record_play_is_idempotent_by_csv_path() {
    let path = temp_db("plays-idem");
    let store = Store::open(&path).unwrap();
    let csv = r"C:\stats\VT Pasu Novice S5 - Challenge - 2023.11.14-19.20.00 Stats.csv";
    let first = play(
        "VT Pasu Novice S5",
        ts(1_700_000_000),
        959.120239,
        145,
        239.5,
    );
    assert!(store.record_play(SID, &first, csv).unwrap(), "first insert");
    // Same file re-ingested with mutated values: ignored, original preserved.
    let mutated = play("VT Pasu Novice S5", ts(1_700_000_000), 111.0, 1, 1.0);
    assert!(
        !store.record_play(SID, &mutated, csv).unwrap(),
        "duplicate csv_path must be a detected no-op"
    );
    let rows = store.plays_history(SID, "VT Pasu Novice S5").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].score, 959.120239, "original row must survive");
    // A different file for the same player+scenario is a distinct row.
    assert!(store
        .record_play(SID, &first, r"C:\stats\copy.csv")
        .unwrap());
    assert_eq!(
        store.plays_history(SID, "VT Pasu Novice S5").unwrap().len(),
        2
    );
    cleanup_db(&path);
}

#[test]
fn plays_history_is_ordered_by_played_at() {
    let path = temp_db("plays-order");
    let store = Store::open(&path).unwrap();
    let late = play("VT Pasu Novice S5", ts(1_700_000_500), 900.0, 140, 240.0);
    let early = play("VT Pasu Novice S5", ts(1_700_000_100), 800.0, 130, 239.0);
    assert!(store.record_play(SID, &late, r"C:\stats\late.csv").unwrap());
    assert!(store
        .record_play(SID, &early, r"C:\stats\early.csv")
        .unwrap());
    let rows = store.plays_history(SID, "VT Pasu Novice S5").unwrap();
    let times: Vec<DateTime<Utc>> = rows.iter().map(|p| p.played_at).collect();
    assert_eq!(times, vec![ts(1_700_000_100), ts(1_700_000_500)]);
    cleanup_db(&path);
}

// ---------- benchmarks_playing ----------

#[test]
fn upsert_played_updates_flags_and_played_benchmarks_filters() {
    let path = temp_db("played");
    let store = Store::open(&path).unwrap();
    store
        .upsert_played(SID, 459, false, ts(1_700_000_000))
        .unwrap();
    assert!(store.played_benchmarks(SID).unwrap().is_empty());
    store
        .upsert_played(SID, 459, true, ts(1_700_001_000))
        .unwrap();
    store
        .upsert_played(SID, 458, true, ts(1_700_001_000))
        .unwrap();
    assert_eq!(store.played_benchmarks(SID).unwrap(), vec![458, 459]);
    // Flag update removes it from the played set.
    store
        .upsert_played(SID, 459, false, ts(1_700_002_000))
        .unwrap();
    assert_eq!(store.played_benchmarks(SID).unwrap(), vec![458]);
    // Other steam ids are isolated by the composite PK.
    store
        .upsert_played("76561190000000000", 459, true, ts(1_700_000_000))
        .unwrap();
    assert_eq!(store.played_benchmarks(SID).unwrap(), vec![458]);
    cleanup_db(&path);
}

// ---------- meta + players ----------

#[test]
fn meta_set_get_and_overwrite() {
    let path = temp_db("meta");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get_meta("first_run_csv_cutoff").unwrap(), None);
    store
        .set_meta("first_run_csv_cutoff", "2026-09-02T00:00:00Z")
        .unwrap();
    assert_eq!(
        store.get_meta("first_run_csv_cutoff").unwrap().as_deref(),
        Some("2026-09-02T00:00:00Z")
    );
    store
        .set_meta("first_run_csv_cutoff", "2026-09-03T00:00:00Z")
        .unwrap();
    assert_eq!(
        store.get_meta("first_run_csv_cutoff").unwrap().as_deref(),
        Some("2026-09-03T00:00:00Z"),
        "set_meta must overwrite"
    );
    assert_eq!(store.get_meta("missing").unwrap(), None);
    cleanup_db(&path);
}

#[test]
fn upsert_player_inserts_and_updates_preserving_first_seen() {
    let path = temp_db("players");
    let store = Store::open(&path).unwrap();
    store.upsert_player(&profile("TestPersona", "FR")).unwrap();
    let p = store.player(SID).unwrap().expect("player row");
    assert_eq!(p.persona, "TestPersona");
    assert_eq!(p.country, "FR");
    let first_seen = |path: &Path| -> String {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.query_row(
            "SELECT first_seen FROM players WHERE steam_id=?1",
            params![SID],
            |r| r.get(0),
        )
        .unwrap()
    };
    let before = first_seen(&path);
    store
        .upsert_player(&profile("TestPersona 2", "FR"))
        .unwrap();
    assert_eq!(first_seen(&path), before, "first_seen must be stable");
    assert_eq!(store.player(SID).unwrap().unwrap().persona, "TestPersona 2");
    assert!(store.player("76561190000000000").unwrap().is_none());
    cleanup_db(&path);
}
