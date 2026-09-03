//! Integration tests for the sync engine (plan Task 1.6).
//!
//! Offline tests drive discovery/sync against a fake `ProgressSource`
//! (no network). The live test (`#[ignore]`) probes kovaaks.com with the
//! verified player id and persists into a throwaway temp DB.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeZone, Utc};
use kovaaks_core::{BenchmarkProgress, Error, ProgressSource, Registry, Store, SyncEngine};

const SID: &str = "76561190000000001";

// ---------- temp-dir helpers (std only) ----------

static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_db(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .subsec_nanos();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "kovaaks-sync-{tag}-{}-{nanos}-{seq}.db",
        std::process::id()
    ))
}

fn cleanup_db(path: &Path) {
    let base = path.to_string_lossy().into_owned();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{base}{suffix}"));
    }
}

// ---------- registry helpers ----------

fn major_ids() -> Vec<i64> {
    let reg = Registry;
    let mut ids: Vec<i64> = reg
        .all()
        .iter()
        .filter(|b| {
            Registry::major_families()
                .iter()
                .any(|f| b.name.starts_with(f))
        })
        .flat_map(|b| b.difficulties.iter().map(|d| d.kovaaks_benchmark_id as i64))
        .collect();
    ids.sort();
    ids
}

fn all_ids() -> Vec<i64> {
    let reg = Registry;
    let mut ids: Vec<i64> = reg
        .all()
        .iter()
        .flat_map(|b| b.difficulties.iter().map(|d| d.kovaaks_benchmark_id as i64))
        .collect();
    ids.sort();
    ids
}

fn some_non_major_id() -> i64 {
    let majors = major_ids();
    all_ids()
        .into_iter()
        .find(|id| !majors.contains(id))
        .expect("registry must contain non-major difficulties")
}

// ---------- fixtures ----------

/// Benchmark progress with a single Tracking scenario scoring `scen_score`.
fn prog(overall: f64, scen_score: f64) -> BenchmarkProgress {
    let scenarios = if scen_score > 0.0 {
        vec![(
            "VT Pasu Novice S5".to_string(),
            kovaaks_core::ScenarioEntry {
                score: scen_score,
                leaderboard_rank: 1,
                scenario_rank: 1,
                rank_maxes: vec![40000.0, 80000.0, 120000.0, 160000.0],
                leaderboard_id: 98059,
            },
        )]
    } else {
        Vec::new()
    };
    BenchmarkProgress {
        benchmark_progress: overall,
        overall_rank: 1,
        categories: vec![(
            "Tracking".to_string(),
            kovaaks_core::CategoryProgress {
                benchmark_progress: overall,
                category_rank: 1,
                rank_maxes: vec![40000.0, 80000.0, 120000.0, 160000.0],
                scenarios,
            },
        )],
    }
}

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .expect("valid timestamp")
}

// ---------- fake ProgressSource ----------

#[derive(Debug, Clone)]
enum FakeReply {
    /// Always succeeds with this progress payload.
    Progress(BenchmarkProgress),
    /// Always fails with a retryable 429.
    RateLimited(u16),
    /// Succeeds only after `failures` retryable failures (attempt <= failures).
    FlakyThenOk { failures: usize },
    /// Sleeps `ms` then returns zero progress (concurrency testing).
    Slow(u64),
}

struct FakeSource {
    replies: HashMap<i64, FakeReply>,
    /// Attempt count per benchmark id (any call, success or failure).
    attempts: Mutex<HashMap<i64, usize>>,
}

impl FakeSource {
    fn new(replies: HashMap<i64, FakeReply>) -> Arc<Self> {
        Arc::new(Self {
            replies,
            attempts: Mutex::new(HashMap::new()),
        })
    }

    fn attempts_for(&self, benchmark_id: i64) -> usize {
        *self
            .attempts
            .lock()
            .unwrap()
            .get(&benchmark_id)
            .unwrap_or(&0)
    }
}

impl ProgressSource for FakeSource {
    fn benchmark_progress(
        &self,
        benchmark_id: i64,
        _steam_id: &str,
    ) -> impl Future<Output = Result<BenchmarkProgress, Error>> + Send {
        {
            let mut attempts = self.attempts.lock().unwrap();
            *attempts.entry(benchmark_id).or_insert(0) += 1;
        }
        let reply = self.replies.get(&benchmark_id).cloned().unwrap_or_else(|| {
            // Unknown ids: zero progress (the common "never played it" case).
            FakeReply::Progress(prog(0.0, 0.0))
        });
        let attempt = self.attempts_for(benchmark_id);
        async move {
            match reply {
                FakeReply::Progress(p) => Ok(p),
                FakeReply::RateLimited(status) => Err(Error::RateLimited { status }),
                FakeReply::FlakyThenOk { failures } => {
                    if attempt <= failures {
                        Err(Error::RateLimited { status: 429 })
                    } else {
                        Ok(prog(0.0, 0.0))
                    }
                }
                FakeReply::Slow(ms) => {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    Ok(prog(0.0, 0.0))
                }
            }
        }
    }
}

/// Replies for every major-family difficulty (zero progress) with selective
/// overrides, so shallow discovery sees a fully-answerable backend.
fn replies_for_majors(overrides: HashMap<i64, FakeReply>) -> HashMap<i64, FakeReply> {
    let mut replies: HashMap<i64, FakeReply> = major_ids()
        .into_iter()
        .map(|id| (id, FakeReply::Progress(prog(0.0, 0.0))))
        .collect();
    for (id, reply) in overrides {
        replies.insert(id, reply);
    }
    replies
}

// ---------- discovery candidates ----------

#[tokio::test]
async fn shallow_discovery_probes_only_major_family_difficulties() {
    let path = temp_db("shallow");
    let store = Store::open(&path).unwrap();
    let majors = major_ids();
    assert!(majors.len() >= 50, "recon: ~70 major-family difficulties");

    let src = FakeSource::new(replies_for_majors(HashMap::from([
        (459, FakeReply::Progress(prog(180000.0, 0.0))), // progress > 0 -> played
        (458, FakeReply::Progress(prog(0.0, 500.0))),    // scenario score > 0 -> played
    ])));
    let engine = SyncEngine::new(store.clone(), src.clone(), &Registry);

    let report = engine.discover(SID, false).await.expect("discovery");
    assert_eq!(
        report.ok,
        majors.len(),
        "every major-family difficulty probed exactly once"
    );
    assert_eq!(report.failed, 0, "errors: {:?}", report.errors);
    // Played semantics: overall progress > 0 OR any scenario score > 0.
    assert_eq!(store.played_benchmarks(SID).unwrap(), vec![458, 459]);
    // Non-major difficulties must NOT have been probed in shallow mode.
    let deep_only = some_non_major_id();
    assert_eq!(
        src.attempts_for(deep_only),
        0,
        "shallow discovery must skip non-major families"
    );
    cleanup_db(&path);
}

#[tokio::test]
async fn deep_discovery_probes_every_registry_difficulty() {
    let path = temp_db("deep");
    let store = Store::open(&path).unwrap();
    let all = all_ids();
    let deep_only = some_non_major_id();
    assert!(all.contains(&deep_only));

    let src = FakeSource::new(replies_for_majors(HashMap::from([(
        // The non-major id answers with a non-retryable 4xx (5xx would be
        // retried per the retry policy).
        deep_only,
        FakeReply::RateLimited(400),
    )])));
    let engine = SyncEngine::new(store.clone(), src.clone(), &Registry);
    let report = engine.discover(SID, true).await.expect("deep discovery");

    // Every difficulty probed exactly once.
    assert_eq!(report.ok + report.failed, all.len());
    assert_eq!(
        src.attempts_for(deep_only),
        1,
        "non-retryable must not retry"
    );
    assert_eq!(report.failed, 1, "only the sentinel id fails");
    assert!(report.errors[0].contains(&deep_only.to_string()));
    cleanup_db(&path);
}

// ---------- retry policy ----------

#[tokio::test]
async fn retries_twice_on_429_then_succeeds() {
    let path = temp_db("retry-ok");
    let store = Store::open(&path).unwrap();
    let src = FakeSource::new(replies_for_majors(HashMap::from([(
        459,
        FakeReply::FlakyThenOk { failures: 2 },
    )])));
    let engine = SyncEngine::new(store.clone(), src.clone(), &Registry);
    let report = engine.discover(SID, false).await.expect("discovery");
    assert_eq!(report.failed, 0, "errors: {:?}", report.errors);
    assert_eq!(
        src.attempts_for(459),
        3,
        "initial attempt + up to 2 retries"
    );
    assert_eq!(report.ok, major_ids().len());
    cleanup_db(&path);
}

#[tokio::test]
async fn gives_up_after_two_retries_and_reports_the_error() {
    let path = temp_db("retry-fail");
    let store = Store::open(&path).unwrap();
    let src = FakeSource::new(replies_for_majors(HashMap::from([(
        459,
        FakeReply::RateLimited(429),
    )])));
    let engine = SyncEngine::new(store.clone(), src.clone(), &Registry);
    let report = engine.discover(SID, false).await.expect("discovery");
    assert_eq!(src.attempts_for(459), 3, "retries stop at 2");
    assert_eq!(report.ok, major_ids().len() - 1);
    assert_eq!(report.failed, 1);
    assert!(
        report.errors[0].contains("459"),
        "error must name the benchmark: {:?}",
        report.errors
    );
    // A failed probe leaves no benchmarks_playing row behind.
    assert!(!store.played_benchmarks(SID).unwrap().contains(&459));
    cleanup_db(&path);
}

// ---------- sync_stale ----------

#[tokio::test]
async fn sync_stale_repulls_only_rows_older_than_max_age() {
    let path = temp_db("stale");
    let store = Store::open(&path).unwrap();
    let now = Utc::now();
    // 459 checked 24h ago (stale at 12h), 458 checked just now (fresh).
    store
        .upsert_played(SID, 459, true, now - chrono::Duration::hours(24))
        .unwrap();
    store.upsert_played(SID, 458, true, now).unwrap();

    let src = FakeSource::new(replies_for_majors(HashMap::from([
        (459, FakeReply::Progress(prog(123456.0, 0.0))),
        (458, FakeReply::Progress(prog(999.0, 0.0))),
    ])));
    let engine = SyncEngine::new(store.clone(), src.clone(), &Registry);
    let report = engine.sync_stale(SID, 12, false).await.expect("sync_stale");

    assert_eq!(report.ok, 1, "only the stale row: {:?}", report.errors);
    assert_eq!(report.failed, 0);
    assert_eq!(src.attempts_for(459), 1);
    assert_eq!(src.attempts_for(458), 0, "fresh row must not be re-pulled");
    // Snapshot recorded with Utc::now().
    let snap = store.latest(SID, 459).unwrap().expect("snapshot recorded");
    assert_eq!(snap.benchmark_progress, 123456);
    let age = Utc::now() - snap.captured_at;
    assert!(age < chrono::Duration::minutes(1), "captured_at is now()");
    // last_checked bumped for the re-checked row...
    let rows = store.benchmarks_playing_rows(SID).unwrap();
    let row_459 = rows.iter().find(|(bid, _, _)| *bid == 459).unwrap();
    assert!(Utc::now() - row_459.2 < chrono::Duration::minutes(1));
    // ...and untouched for the fresh row.
    let row_458 = rows.iter().find(|(bid, _, _)| *bid == 458).unwrap();
    assert_eq!(row_458.2, now);
    cleanup_db(&path);
}

#[tokio::test]
async fn sync_stale_force_repulls_every_row() {
    let path = temp_db("force");
    let store = Store::open(&path).unwrap();
    let now = Utc::now();
    store.upsert_played(SID, 459, true, now).unwrap();
    store.upsert_played(SID, 458, true, now).unwrap();

    let src = FakeSource::new(replies_for_majors(HashMap::from([(
        459,
        FakeReply::Progress(prog(180000.0, 0.0)),
    )])));
    let engine = SyncEngine::new(store.clone(), src.clone(), &Registry);
    let report = engine.sync_stale(SID, 12, true).await.expect("force sync");
    assert_eq!(report.ok, 2, "both rows re-pulled: {:?}", report.errors);
    assert_eq!(src.attempts_for(459), 1);
    assert_eq!(src.attempts_for(458), 1);
    assert!(store.latest(SID, 459).unwrap().is_some());
    assert!(store.latest(SID, 458).unwrap().is_some());
    cleanup_db(&path);
}

// ---------- bounded concurrency ----------

#[tokio::test]
async fn probes_run_bounded_in_parallel_not_serially() {
    let path = temp_db("parallel");
    let store = Store::open(&path).unwrap();
    // Eight stale rows; each probe sleeps 150ms.
    let now = Utc::now();
    let stale_ids: Vec<i64> = major_ids().into_iter().take(8).collect();
    for bid in &stale_ids {
        store.upsert_played(SID, *bid, true, now).unwrap();
    }
    let replies: HashMap<i64, FakeReply> = stale_ids
        .iter()
        .map(|bid| (*bid, FakeReply::Slow(150)))
        .collect();
    let src = FakeSource::new(replies);
    let engine = SyncEngine::new(store.clone(), src, &Registry);

    let started = Instant::now();
    let report = engine.sync_stale(SID, 12, true).await.expect("sync");
    let elapsed = started.elapsed();
    assert_eq!(report.ok, 8);
    // 8 x 150ms serial = 1.2s; 4-way concurrency = ~300ms. Wide margin.
    assert!(
        elapsed < Duration::from_millis(900),
        "probes must run with bounded concurrency (4), took {elapsed:?}"
    );
    cleanup_db(&path);
}

// ---------- live (#[ignore]) ----------

/// Live discovery against kovaaks.com for the verified player. Persisted into
/// a temp DB (never the real app DB). Ground truth recon 2026-09-02:
/// ~110 played cards, VT S5 Novice (459) progress 180000.
#[tokio::test]
#[ignore]
async fn live_discovery_finds_50_plus_played_benchmarks() {
    let path = temp_db("live-discover");
    let store = Store::open(&path).unwrap();
    let client = kovaaks_core::KovaaksClient::new().expect("client");
    let engine = SyncEngine::new(store.clone(), client, &Registry);

    let started = Instant::now();
    let report = engine.discover(SID, false).await.expect("live discovery");
    println!(
        "discover: ok={} failed={} in {:?} errors={:?}",
        report.ok,
        report.failed,
        started.elapsed(),
        report.errors
    );
    assert_eq!(
        report.failed, 0,
        "all probes must succeed: {:?}",
        report.errors
    );
    assert_eq!(report.ok, major_ids().len());

    let played = store.played_benchmarks(SID).unwrap();
    println!("played benchmarks: {}", played.len());
    assert!(
        played.len() >= 50,
        "expected >= 50 played benchmarks, got {}",
        played.len()
    );
    assert!(played.contains(&459), "VT S5 Novice must be among them");

    // Sync pass over everything just discovered (stale = everything, since
    // discover bumped last_checked only; force to re-pull all rows).
    let started = Instant::now();
    let report = engine.sync_stale(SID, 12, true).await.expect("live sync");
    println!(
        "sync_stale: ok={} failed={} in {:?} errors={:?}",
        report.ok,
        report.failed,
        started.elapsed(),
        report.errors
    );
    assert_eq!(report.failed, 0, "sync errors: {:?}", report.errors);
    let snap = store.latest(SID, 459).unwrap().expect("459 snapshot");
    assert!(
        snap.benchmark_progress >= 100000,
        "VT S5 Novice progress ground truth is 180000, got {}",
        snap.benchmark_progress
    );
    cleanup_db(&path);
}
