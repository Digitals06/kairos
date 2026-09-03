//! Sync engine + played-benchmark discovery (plan Task 1.6).
//!
//! Discovery probes the KovaaK's webapp-backend `benchmark_progress` endpoint
//! for candidate difficulties (major families by default, the whole registry
//! on deep scans) and persists played flags into `benchmarks_playing`. The
//! sync pass re-pulls stale `benchmarks_playing` rows and records snapshots.
//!
//! HTTP access is abstracted behind the [`ProgressSource`] trait so tests can
//! fake the backend; [`KovaaksClient`] is the concrete implementation.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::stream::{self, StreamExt};
use tokio::sync::Semaphore;

use crate::error::{Error, Result};
use crate::kovaaks::KovaaksClient;
use crate::registry::Registry;
use crate::store::Store;
use crate::types::BenchmarkProgress;

/// Probe concurrency (plan: conservative 4 req/s).
const PROBE_CONCURRENCY: usize = 4;
/// Retries per probe on 429/5xx (initial attempt excluded).
const MAX_RETRIES: usize = 2;
/// Base backoff between retries; jitter widens it (rand-free).
const BACKOFF_BASE_MS: u64 = 250;
/// Cap on the sleep between attempts.
const BACKOFF_CAP_MS: u64 = 4000;

/// Outcome of a discovery or sync pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Benchmarks successfully probed/recorded.
    pub ok: usize,
    /// Benchmarks that failed after retries.
    pub failed: usize,
    /// One human-readable entry per failed benchmark.
    pub errors: Vec<String>,
}

/// Anything that can fetch one player's progress on one benchmark.
///
/// Implemented by [`KovaaksClient`] (live) and by test fakes. Returns
/// `impl Future` so fakes need no boxing.
pub trait ProgressSource: Send + Sync {
    fn benchmark_progress(
        &self,
        benchmark_id: i64,
        steam_id: &str,
    ) -> impl Future<Output = Result<BenchmarkProgress>> + Send;
}

/// Blanket impl so `Arc<S>` (and other dere wrappers) work as a source.
impl<T: ProgressSource + ?Sized> ProgressSource for Arc<T> {
    fn benchmark_progress(
        &self,
        benchmark_id: i64,
        steam_id: &str,
    ) -> impl Future<Output = Result<BenchmarkProgress>> + Send {
        (**self).benchmark_progress(benchmark_id, steam_id)
    }
}

impl ProgressSource for KovaaksClient {
    fn benchmark_progress(
        &self,
        benchmark_id: i64,
        steam_id: &str,
    ) -> impl Future<Output = Result<BenchmarkProgress>> + Send {
        KovaaksClient::benchmark_progress(self, benchmark_id, steam_id)
    }
}

/// Whether one fetch needs a retry (429 or 5xx — transient by policy).
fn is_retryable(err: &Error) -> bool {
    matches!(
        err,
        Error::RateLimited {
            status: 429 | 500..=599
        }
    )
}

/// Jittered backoff sleep before retry attempt `retry` (1-based). Rand-free:
/// the jitter component derives from `SystemTime` sub-second nanos.
async fn backoff_sleep(retry: usize) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let jitter = (nanos % 100) as u64;
    let base = BACKOFF_BASE_MS << (retry - 1).min(4);
    let sleep_ms = (base + jitter).min(BACKOFF_CAP_MS);
    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
}

/// Runs one benchmark probe with up to [`MAX_RETRIES`] retries on 429/5xx.
async fn probe_with_retry(
    source: &impl ProgressSource,
    benchmark_id: i64,
    steam_id: &str,
) -> Result<BenchmarkProgress> {
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match source.benchmark_progress(benchmark_id, steam_id).await {
            Ok(progress) => return Ok(progress),
            Err(e) if is_retryable(&e) && attempt <= MAX_RETRIES => {
                backoff_sleep(attempt).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Whether probe results write played flags (discovery) or snapshots (sync).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistMode {
    Played,
    Snapshot,
}

/// Sync engine: discovery + snapshot sync over one store and one source.
/// Cheap to clone.
#[derive(Clone)]
pub struct SyncEngine<S: ProgressSource> {
    store: Store,
    source: S,
    registry: Registry,
    /// Bounds probe concurrency (shared by clones of this engine).
    semaphore: Arc<Semaphore>,
}

impl<S: ProgressSource> SyncEngine<S> {
    /// Build an engine over a store, a progress source and the registry.
    pub fn new(store: Store, source: S, registry: &Registry) -> Self {
        Self {
            store,
            source,
            registry: *registry,
            semaphore: Arc::new(Semaphore::new(PROBE_CONCURRENCY)),
        }
    }

    /// Candidate difficulty ids for a discovery pass. `deep=false` probes only
    /// difficulties whose benchmark name starts with one of the registry's
    /// `major_families()`; `deep=true` probes every registry difficulty.
    pub fn candidate_ids(&self, deep: bool) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .registry
            .all()
            .iter()
            .filter(|b| {
                deep || Registry::major_families()
                    .iter()
                    .any(|f| b.name.starts_with(f))
            })
            .flat_map(|b| b.difficulties.iter().map(|d| d.kovaaks_benchmark_id as i64))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Probe candidate difficulties and persist played flags.
    ///
    /// A benchmark counts as played when `benchmark_progress > 0` OR any
    /// scenario score > 0. Probes run with bounded concurrency (4) and retry
    /// on 429/5xx (up to 2, jittered backoff). Failures are reported per
    /// benchmark in [`SyncReport::errors`] and leave no row behind.
    pub async fn discover(&self, steam_id: &str, deep: bool) -> Result<SyncReport> {
        KovaaksClient::validate_steam_id(steam_id)?;
        let ids = self.candidate_ids(deep);
        let ids = ids.into_iter().map(|id| (id, false)).collect();
        Ok(self.probe_all(steam_id, ids, PersistMode::Played).await)
    }

    /// Re-pull `benchmarks_playing` rows whose `last_checked` is older than
    /// `max_age_hours` (every row when `force`), recording a snapshot per row
    /// with `chrono::Utc::now()` as `captured_at` and bumping `last_checked`.
    pub async fn sync_stale(
        &self,
        steam_id: &str,
        max_age_hours: u64,
        force: bool,
    ) -> Result<SyncReport> {
        KovaaksClient::validate_steam_id(steam_id)?;
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours as i64);
        let rows: Vec<(i64, bool)> = self
            .store
            .benchmarks_playing_rows(steam_id)?
            .into_iter()
            .filter(|(_, _, last_checked)| force || *last_checked < cutoff)
            .map(|(benchmark_id, played, _)| (benchmark_id, played))
            .collect();
        Ok(self.probe_all(steam_id, rows, PersistMode::Snapshot).await)
    }

    /// Probe a list of benchmark ids with bounded concurrency and collect a
    /// [`SyncReport`]. Each entry is `(benchmark_id, current_played_flag)`;
    /// the flag is preserved on snapshot writes and recomputed on discovery.
    async fn probe_all(
        &self,
        steam_id: &str,
        ids: Vec<(i64, bool)>,
        mode: PersistMode,
    ) -> SyncReport {
        let results: Vec<(i64, bool, Result<BenchmarkProgress>)> = stream::iter(ids)
            .map(|(benchmark_id, played_flag)| async move {
                let _permit = self.permit().await;
                let result = probe_with_retry(&self.source, benchmark_id, steam_id).await;
                (benchmark_id, played_flag, result)
            })
            .buffer_unordered(PROBE_CONCURRENCY)
            .collect()
            .await;

        let mut report = SyncReport {
            ok: 0,
            failed: 0,
            errors: Vec::new(),
        };
        for (benchmark_id, played_flag, result) in results {
            match result {
                Ok(progress) => {
                    report.ok += 1;
                    match mode {
                        PersistMode::Played => {
                            let played = progress.benchmark_progress > 0.0
                                || progress
                                    .categories
                                    .iter()
                                    .flat_map(|(_, c)| c.scenarios.iter())
                                    .any(|(_, s)| s.score > 0.0);
                            self.store
                                .upsert_played(steam_id, benchmark_id, played, Utc::now())
                                .expect("store write must succeed");
                        }
                        PersistMode::Snapshot => {
                            self.store
                                .upsert_played(steam_id, benchmark_id, played_flag, Utc::now())
                                .expect("store write must succeed");
                            self.store
                                .record_snapshot(steam_id, benchmark_id, &progress, Utc::now())
                                .expect("store write must succeed");
                        }
                    }
                }
                Err(e) => {
                    report.failed += 1;
                    report.errors.push(format!("benchmark {benchmark_id}: {e}"));
                }
            }
        }
        report
    }

    /// Acquire one of the shared probe-concurrency permits.
    async fn permit(&self) -> tokio::sync::OwnedSemaphorePermit {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore is never closed")
    }
}
