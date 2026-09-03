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
/// Leftover pass: cooldown before re-probing throttled benchmarks. A larger
/// server `Retry-After` hint wins (capped by [`LEFTOVER_COOLDOWN_CAP`]).
const LEFTOVER_COOLDOWN: Duration = Duration::from_secs(15);
/// Upper bound for a server-driven leftover cooldown.
const LEFTOVER_COOLDOWN_CAP: Duration = Duration::from_secs(120);
/// Leftover pass: retries per benchmark (initial reprobe excluded). The pass
/// runs sequentially, so this budget costs time, not throttle pressure.
const LEFTOVER_RETRIES: usize = 3;
/// Leftover pass: backoff base/cap between reprobes (slower than the main
/// pass — the server just told us to back off).
const LEFTOVER_BACKOFF_BASE_MS: u64 = 2000;
const LEFTOVER_BACKOFF_CAP_MS: u64 = 30000;

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
/// Returns the server's `Retry-After` hint (seconds) when retryable.
fn retry_hint(err: &Error) -> Option<Option<u64>> {
    match err {
        Error::RateLimited {
            status: 429 | 500..=599,
            retry_after_secs,
        } => Some(*retry_after_secs),
        _ => None,
    }
}

/// Jittered backoff sleep before retry attempt `retry` (1-based). Rand-free:
/// the jitter component derives from `SystemTime` sub-second nanos.
async fn backoff_sleep(retry: usize, base_ms: u64, cap_ms: u64) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let jitter = (nanos % 100) as u64;
    let base = base_ms << (retry - 1).min(4);
    let sleep_ms = (base + jitter).min(cap_ms);
    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
}

/// Runs one benchmark probe with up to `max_retries` retries on 429/5xx.
async fn probe_with_retry_opts(
    source: &impl ProgressSource,
    benchmark_id: i64,
    steam_id: &str,
    max_retries: usize,
    base_ms: u64,
    cap_ms: u64,
) -> Result<BenchmarkProgress> {
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match source.benchmark_progress(benchmark_id, steam_id).await {
            Ok(progress) => return Ok(progress),
            Err(e) if retry_hint(&e).is_some() && attempt <= max_retries => {
                backoff_sleep(attempt, base_ms, cap_ms).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Runs one benchmark probe with up to [`MAX_RETRIES`] retries on 429/5xx.
async fn probe_with_retry(
    source: &impl ProgressSource,
    benchmark_id: i64,
    steam_id: &str,
) -> Result<BenchmarkProgress> {
    probe_with_retry_opts(
        source,
        benchmark_id,
        steam_id,
        MAX_RETRIES,
        BACKOFF_BASE_MS,
        BACKOFF_CAP_MS,
    )
    .await
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
    /// Cooldown before the leftover pass (honor a larger server hint).
    leftover_cooldown: Duration,
    /// Retries per benchmark in the leftover pass (sequential reprobe).
    leftover_retries: usize,
}

impl<S: ProgressSource> SyncEngine<S> {
    /// Build an engine over a store, a progress source and the registry.
    pub fn new(store: Store, source: S, registry: &Registry) -> Self {
        Self {
            store,
            source,
            registry: *registry,
            semaphore: Arc::new(Semaphore::new(PROBE_CONCURRENCY)),
            leftover_cooldown: LEFTOVER_COOLDOWN,
            leftover_retries: LEFTOVER_RETRIES,
        }
    }

    /// Override the leftover-pass policy (cooldown before the sequential
    /// re-probe, retries per leftover benchmark). Tests use zero cooldowns
    /// to keep the suite fast; production keeps the defaults from [`new`].
    pub fn with_leftover_policy(mut self, cooldown: Duration, retries: usize) -> Self {
        self.leftover_cooldown = cooldown;
        self.leftover_retries = retries;
        self
    }

    /// Candidate difficulty ids for a discovery pass.
    ///
    /// evxl shows every visible benchmark's difficulties on a user page, so
    /// `deep=true` probes exactly those (matching evxl's coverage). The
    /// default pass (`deep=false`) is a fast subset — the major community
    /// families — while `Deep Scan` extends to everything evxl tracks.
    pub fn candidate_ids(&self, deep: bool) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .registry
            .visible()
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
    /// on 429/5xx (up to 2, jittered backoff); throttled leftovers get a
    /// cooled-down sequential second pass. Failures are reported per
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
    ///
    /// Throttled leftovers (429/5xx after the main pass) get a second chance:
    /// after a cooldown (configured, or the server's `Retry-After` hint when
    /// larger) they are re-probed one at a time with a slower backoff.
    /// Terminal errors (4xx, decode failures) report immediately — retrying
    /// those would only burn time.
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
        // Retryable leftovers: (benchmark_id, played_flag, hint_secs).
        let mut leftovers: Vec<(i64, bool, u64)> = Vec::new();
        for (benchmark_id, played_flag, result) in results {
            match result {
                Ok(progress) => {
                    report.ok += 1;
                    self.persist(steam_id, benchmark_id, played_flag, &progress, mode);
                }
                Err(e) => match retry_hint(&e) {
                    Some(hint) => leftovers.push((benchmark_id, played_flag, hint.unwrap_or(0))),
                    None => {
                        report.failed += 1;
                        report.errors.push(format!("benchmark {benchmark_id}: {e}"));
                    }
                },
            }
        }
        if !leftovers.is_empty() {
            self.probe_leftovers(steam_id, leftovers, mode, &mut report)
                .await;
        }
        report
    }

    /// Persist one successful probe result (played flag or snapshot row).
    fn persist(
        &self,
        steam_id: &str,
        benchmark_id: i64,
        played_flag: bool,
        progress: &BenchmarkProgress,
        mode: PersistMode,
    ) {
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
                    .record_snapshot(steam_id, benchmark_id, progress, Utc::now())
                    .expect("store write must succeed");
            }
        }
    }

    /// Second chance for throttled benchmarks: cool down (configured, or the
    /// largest server `Retry-After` hint when bigger), then re-probe strictly
    /// sequentially with a slower backoff. Recovered probes merge into
    /// `report.ok`; only the final survivors stay in `report.errors`.
    async fn probe_leftovers(
        &self,
        steam_id: &str,
        leftovers: Vec<(i64, bool, u64)>,
        mode: PersistMode,
        report: &mut SyncReport,
    ) {
        let hint_max = leftovers
            .iter()
            .map(|(_, _, hint)| *hint)
            .max()
            .unwrap_or(0);
        let cooldown = self.leftover_cooldown.max(Duration::from_secs(
            hint_max.min(LEFTOVER_COOLDOWN_CAP.as_secs()),
        ));
        if !cooldown.is_zero() {
            tokio::time::sleep(cooldown).await;
        }
        for (benchmark_id, played_flag, _) in leftovers {
            match probe_with_retry_opts(
                &self.source,
                benchmark_id,
                steam_id,
                self.leftover_retries,
                LEFTOVER_BACKOFF_BASE_MS,
                LEFTOVER_BACKOFF_CAP_MS,
            )
            .await
            {
                Ok(progress) => {
                    report.ok += 1;
                    self.persist(steam_id, benchmark_id, played_flag, &progress, mode);
                }
                Err(e) => {
                    report.failed += 1;
                    report.errors.push(format!("benchmark {benchmark_id}: {e}"));
                }
            }
        }
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
