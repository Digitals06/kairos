//! Kairos — KovaaK's Companion: Tauri v2 app shell + commands bridge.
//!
//! Every UI-facing operation is a `#[tauri::command]` backed by
//! `kovaaks-core`; the frontend never talks to the network or SQLite
//! directly. Offline-first: reads come from the local store; network
//! happens only in `resolve_profile` / `sync_now`.

use std::{collections::BTreeMap, path::PathBuf, time::Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

use kovaaks_core::{
    csv_ingest, metrics_for_benchmark, metrics_for_scenario_combined,
    rankdiff::RankChange,
    store::StoredSnapshot,
    types::{Difficulty, RankTier},
    EvxlClient, KovaaksClient, Registry, Store, SyncEngine, SyncReport,
};

/// Meta key: JSON blob of app settings (stats dir override, sync interval).
const SETTINGS_KEY: &str = "settings";
/// Meta key: stats `csv_seen` counter from the last ingest scan.
const INGEST_SEEN_KEY: &str = "ingest_stats_csv_seen";
/// Meta key: stats `csv_inserted` counter from the last ingest scan.
const INGEST_INSERTED_KEY: &str = "ingest_stats_csv_inserted";
/// Meta key: RFC3339 timestamp of the last successful `sync_now`.
const LAST_SYNCED_KEY: &str = "last_synced_at";
/// Default KovaaK's stats dir (standard Steam library install).
const DEFAULT_STATS_DIR: &str =
    "C:\\Program Files (x86)\\Steam\\steamapps\\common\\FPSAimTrainer\\FPSAimTrainer\\stats";
/// Sync staleness threshold (hours): rows older than this get re-probed on
/// every `sync_now` (and `force=true` re-probes everything regardless).
const SYNC_MAX_AGE_HOURS: u64 = 2;

// ---------------------------------------------------------------------------
// UI-facing DTOs (serde snake_case mirrors of the TypeScript interfaces in
// ui/src/lib/api.ts — the whole frontend reads snake_case fields; camelCase
// here once crashed every card render with `undefined.toLocaleString`).
// REGRESSION: dto_wire_format_is_snake_case guards this.
// ---------------------------------------------------------------------------

/// One benchmark row on the overview grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BenchmarkCard {
    pub benchmark_id: i64,
    pub benchmark_name: String,
    pub abbreviation: String,
    pub difficulty_name: String,
    pub rank: Option<RankTier>,
    pub benchmark_progress: i64,
    pub next_rank_name: Option<String>,
    pub next_rank_delta: Option<i64>,
    pub avg_score: f64,
    pub high_score: f64,
    pub avg_improvement_pct: Option<f64>,
    pub high_improvement_pct: Option<f64>,
    pub samples: usize,
    pub last_synced: Option<String>,
    /// Whether this benchmark is pinned to the top of the player's list.
    pub is_favorite: bool,
    /// Full snapshot history so the UI can draw sparklines without an N+1 of
    /// per-card detail calls (those starve the main thread on 70-card grids).
    pub snapshot_history: Vec<SnapshotPoint>,
}

/// One scenario row in the benchmark detail view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScenarioRank {
    pub scenario: String,
    pub score: i64,
    pub leaderboard_rank: i64,
    pub tier: Option<RankTier>,
    /// 1-based achieved tier index from the API (0 = unplayed).
    pub scenario_rank: i64,
    /// This scenario's tier thresholds (display units), ascending.
    pub rank_maxes: Vec<f64>,
}

/// One scenario's score history across snapshots (per-scenario trends).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScenarioHistorySeries {
    pub scenario: String,
    pub category: String,
    /// "snapshot" (synced scores) or "local" (CSV plays — no synced score).
    pub source: String,
    pub points: Vec<ScenarioHistoryPoint>,
}

/// One (time, score) point of a scenario's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScenarioHistoryPoint {
    pub captured_at: String,
    pub score: i64,
}

/// One category row in the benchmark detail view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CategoryCard {
    pub name: String,
    pub progress: i64,
    pub rank_tier: Option<RankTier>,
}

/// One snapshot-history point.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SnapshotPoint {
    pub captured_at: String,
    pub benchmark_progress: i64,
}

/// One CSV play point.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlayPoint {
    /// Scenario this play belongs to (scoped chart underlay).
    pub scenario: String,
    pub played_at: String,
    pub score: f64,
}

/// Full detail for one benchmark: card + snapshot history + CSV plays +
/// scenario tiers + per-category progress from the newest snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BenchmarkDetail {
    pub card: BenchmarkCard,
    pub snapshot_history: Vec<SnapshotPoint>,
    pub plays: Vec<PlayPoint>,
    pub scenario_ranks: Vec<ScenarioRank>,
    pub categories: Vec<CategoryCard>,
    /// Per-scenario score history across snapshots (scenario-scoped trends).
    pub scenario_history: Vec<ScenarioHistorySeries>,
    /// The difficulty's rank ladder (name + color, worst → best) for the
    /// evxl-style per-tier threshold matrix.
    pub rank_tiers: Vec<RankTier>,
    /// Per-scenario improvement metrics (avg/high/improvement from CSV plays),
    /// keyed by scenario name — powers the stat cards when a scenario is
    /// selected in the chart picker.
    pub scenario_metrics: std::collections::BTreeMap<String, kovaaks_core::Metrics>,
}

/// Counters from the last CSV ingest scan + last sync time.
///
/// `last_synced_at` is an additive extension of the planned shape; the
/// overview top bar uses it for the last-synced display and the 12h stale
/// badge without a dedicated extra command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct IngestStatus {
    pub csv_seen: u64,
    pub csv_inserted: u64,
    pub last_synced_at: Option<String>,
}

/// Wire mirror of `kovaaks_core::SyncReport` (core types stay serde-free
/// of UI concerns; core's struct does not derive Serialize).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncReportDto {
    pub ok: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

impl From<SyncReport> for SyncReportDto {
    fn from(r: SyncReport) -> Self {
        Self {
            ok: r.ok,
            failed: r.failed,
            errors: r.errors,
        }
    }
}

/// App settings (persisted as a JSON blob in the store's meta table).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", default)]
pub struct AppSettings {
    /// Stats dir override; empty string = auto-detect (default Steam path).
    pub stats_dir: String,
    /// Background sync interval in hours (0 = only manual sync).
    pub sync_interval_hours: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            stats_dir: String::new(),
            sync_interval_hours: 6,
        }
    }
}

// ---------------------------------------------------------------------------
// Ladder / rank estimation helpers
// ---------------------------------------------------------------------------

/// Ascending, deduplicated i64 view of a snapshot row's stored `rank_maxes`.
fn ladder_from_rows(rank_maxes: &[f64]) -> Vec<i64> {
    let mut maxes: Vec<i64> = rank_maxes.iter().map(|m| *m as i64).collect();
    maxes.sort_unstable();
    maxes.dedup();
    maxes
}

/// Overall rank ladder estimated from the latest snapshot.
///
/// The webapp-backend only publishes per-category ladders (`rank_maxes`);
/// the overall aggregation lives server-side. Because category progress is
/// additive, the elementwise sum of the per-category ladders is a faithful
/// overall estimate whenever every category publishes the same tier count;
/// otherwise fall back to the longest single ladder. Deltas computed on
/// this ladder are estimates, refined in Task 3.x with live rank payloads.
fn overall_ladder(snapshot: Option<&StoredSnapshot>) -> Vec<i64> {
    let Some(snap) = snapshot else {
        return Vec::new();
    };
    let mut per_category: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for row in &snap.scenarios {
        per_category
            .entry(row.category.as_str())
            .or_insert_with(|| ladder_from_rows(&row.rank_maxes));
    }
    let ladders: Vec<Vec<i64>> = per_category.into_values().collect();
    if ladders.is_empty() {
        return Vec::new();
    }
    let first_len = ladders[0].len();
    if first_len > 0 && ladders.iter().all(|l| l.len() == first_len) {
        (0..first_len)
            .map(|i| ladders.iter().map(|l| l[i]).sum())
            .collect()
    } else {
        ladders
            .iter()
            .max_by_key(|l| l.len())
            .map(|l| (*l).clone())
            .unwrap_or_default()
    }
}

/// Next tier + remaining delta: the minimum ladder threshold strictly above
/// `progress`; the tier name comes from the difficulty ladder at that index.
/// No threshold above progress (top tier) or no data → `(None, None)`.
fn next_rank_from_ladder(
    progress: i64,
    ladder: &[i64],
    difficulty: &Difficulty,
) -> (Option<String>, Option<i64>) {
    match ladder.iter().find(|&&t| t > progress) {
        Some(&threshold) => {
            let idx = ladder.iter().position(|&t| t == threshold).unwrap_or(0);
            let name = difficulty.rank_colors.get(idx).map(|t| t.name.clone());
            (name, Some(threshold - progress))
        }
        None => (None, None),
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

/// Shared app state handed to every command. The async runtime is Tauri's
/// global `tauri::async_runtime` (tokio multi-thread), so no handle field
/// is needed; `Store` is `Clone` (connection behind a mutex) and `Send + Sync`.
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub registry: &'static Registry,
    pub evxl: EvxlClient,
}

impl AppState {
    /// Current player profile, if connected.
    fn profile(&self) -> kovaaks_core::Result<Option<kovaaks_core::types::PlayerProfile>> {
        match self.store.get_meta("steam_id")? {
            Some(id) if !id.is_empty() => self.store.player(&id),
            _ => Ok(None),
        }
    }

    /// Settings JSON blob from meta (defaults when absent or corrupt).
    fn load_settings(&self) -> AppSettings {
        self.store
            .get_meta(SETTINGS_KEY)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    /// Resolve the active stats dir: settings override or default path.
    fn stats_dir(&self) -> PathBuf {
        let settings = self.load_settings();
        if settings.stats_dir.trim().is_empty() {
            PathBuf::from(DEFAULT_STATS_DIR)
        } else {
            PathBuf::from(settings.stats_dir.trim())
        }
    }
}

/// SQLite DB path: `%LOCALAPPDATA%\kairos\store.db` (dir created).
/// First launch after the rename carries the legacy
/// `%LOCALAPPDATA%\kovaaks-companion\store.db` forward (history + favorites),
/// so the rename never orphans player data. A failed move falls back to a
/// fresh DB rather than refusing to start.
fn db_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").map_or_else(
        |_| {
            std::env::var("USERPROFILE")
                .map(|h| PathBuf::from(h).join("AppData").join("Local"))
                .unwrap_or_else(|_| PathBuf::from("."))
        },
        PathBuf::from,
    );
    let dir = base.join("kairos");
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("store.db");
    if !db.exists() {
        let legacy = base.join("kovaaks-companion").join("store.db");
        if legacy.exists() {
            let _ = std::fs::rename(&legacy, &db);
        }
    }
    db
}

/// CSV ingest scan (forward-only from the stored first-run cutoff). Tolerates
/// a missing KovaaK's install — scan errors land on stderr and the counters
/// stay at their previous values. Runs without a profile as a no-op; called
/// again from `resolve_profile` once a player is connected.
fn run_csv_scan(store: &Store) -> Option<(usize, usize)> {
    let steam_id = match store.get_meta("steam_id") {
        Ok(Some(id)) if !id.is_empty() => id,
        _ => return None,
    };
    let dir = {
        let settings: Option<AppSettings> = store
            .get_meta(SETTINGS_KEY)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok());
        match settings {
            Some(s) if !s.stats_dir.trim().is_empty() => PathBuf::from(s.stats_dir.trim()),
            _ => PathBuf::from(DEFAULT_STATS_DIR),
        }
    };
    let cutoff = match csv_ingest::ensure_cutoff(store) {
        Ok(c) => c,
        Err(_) => return None,
    };
    let report = csv_ingest::scan_dir(&dir, cutoff, store, &steam_id);
    let _ = store.set_meta(INGEST_SEEN_KEY, &report.seen.to_string());
    let _ = store.set_meta(INGEST_INSERTED_KEY, &report.inserted.to_string());
    for e in report.errors.iter().take(5) {
        eprintln!("csv ingest: {e}");
    }
    Some((report.seen, report.inserted))
}

/// Ingest counters for the DTO, read back from the stored meta.
fn ingest_status_from(store: &Store) -> IngestStatus {
    let meta_num = |key: &str| -> u64 {
        store
            .get_meta(key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    IngestStatus {
        csv_seen: meta_num(INGEST_SEEN_KEY),
        csv_inserted: meta_num(INGEST_INSERTED_KEY),
        last_synced_at: store.get_meta(LAST_SYNCED_KEY).ok().flatten(),
    }
}

pub mod commands {
    use super::*;

    // ---------------------------------------------------------------------------
    // Commands
    // ---------------------------------------------------------------------------

    /// Resolve a SteamID64 / vanity URL / profile URL via evxl and persist it
    /// (players table + `steam_id` meta), then kick the CSV scan for this player.
    #[tauri::command]
    pub async fn resolve_profile(
        state: State<'_, AppState>,
        identifier: String,
    ) -> Result<kovaaks_core::types::PlayerProfile, String> {
        let profile = state
            .evxl
            .resolve(identifier.trim())
            .await
            .map_err(|e| e.to_string())?;
        state
            .store
            .upsert_player(&profile)
            .map_err(|e| e.to_string())?;
        state
            .store
            .set_meta("steam_id", &profile.steam_id)
            .map_err(|e| e.to_string())?;
        let store = state.store.clone();
        tauri::async_runtime::spawn_blocking(move || run_csv_scan(&store));
        Ok(profile)
    }

    /// The connected player profile, if any.
    #[tauri::command]
    pub fn get_profile(
        state: State<'_, AppState>,
    ) -> Result<Option<kovaaks_core::types::PlayerProfile>, String> {
        state.profile().map_err(|e| e.to_string())
    }

    /// Sync played benchmarks + discovery over the major families (or the whole
    /// registry with `deep`), then refresh the CSV scan. Discovery writes played
    /// flags; the forced stale pass records snapshots for every known row
    /// (including ones discovery just found), so one call leaves the store fully
    /// populated. Combined report; RFC3339 timestamp lands in meta on success.
    #[tauri::command]
    pub async fn sync_now(state: State<'_, AppState>, deep: bool) -> Result<SyncReportDto, String> {
        let started = Instant::now();
        let (store, steam_id) = match state.profile().map_err(|e| e.to_string())? {
            Some(p) => (state.store.clone(), p.steam_id),
            None => return Err("no profile connected".into()),
        };
        let source = KovaaksClient::new().map_err(|e| e.to_string())?;
        let engine = SyncEngine::new(store.clone(), source, state.registry);
        let discovery = engine
            .discover(&steam_id, deep)
            .await
            .map_err(|e| e.to_string())?;
        let stale = engine
            .sync_stale(&steam_id, SYNC_MAX_AGE_HOURS, true)
            .await
            .map_err(|e| e.to_string())?;
        let report = SyncReportDto {
            ok: discovery.ok + stale.ok,
            failed: discovery.failed + stale.failed,
            errors: discovery.errors.into_iter().chain(stale.errors).collect(),
        };
        // Cheap, idempotent local refresh alongside the network sync.
        let dir = state.stats_dir();
        if let Ok(cutoff) = csv_ingest::ensure_cutoff(&store) {
            let scan = csv_ingest::scan_dir(&dir, cutoff, &store, &steam_id);
            let _ = store.set_meta(INGEST_SEEN_KEY, &scan.seen.to_string());
            let _ = store.set_meta(INGEST_INSERTED_KEY, &scan.inserted.to_string());
        }
        let _ = store.set_meta(LAST_SYNCED_KEY, &chrono::Utc::now().to_rfc3339());
        eprintln!(
            "sync_now: ok={} failed={} in {:?}",
            report.ok,
            report.failed,
            started.elapsed()
        );
        Ok(report)
    }

    /// Build one overview card from store + registry + metrics.
    ///
    /// Rank comes from the API's own `overall_rank` index (authoritative —
    /// each benchmark ranks by its own `rankCalculation` server-side);
    /// progress/deltas are display-unit values (client already normalized).
    /// Next-rank delta uses the category ladder sum (same additive model the
    /// API's `benchmark_progress` uses) — now in the same units, so deltas
    /// are meaningful.
    /// Rebuild an API-shaped [`BenchmarkProgress`] from a stored snapshot so
    /// the rank engine can consume it. Stored scores and rank_maxes are
    /// display units (already ÷100 from the API); the engine works in display
    /// units too, so no scaling. Scenario order follows the stored
    /// `api_order` (category × 10 000 + index), matching evxl's ordering.
    fn stored_to_progress(
        snap: &kovaaks_core::store::StoredSnapshot,
    ) -> kovaaks_core::types::BenchmarkProgress {
        use kovaaks_core::types::{CategoryProgress, ScenarioEntry};
        let mut categories: Vec<(String, CategoryProgress)> = Vec::new();
        // Stored rows are ordered by (category, order_idx); rebuild the API's
        // per-category scenario maps in that same document order.
        for row in &snap.scenarios {
            // Stored scores and rank_maxes are display units (already ÷100
            // from the API); the engine works in display units too — no scaling.
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
        kovaaks_core::types::BenchmarkProgress {
            benchmark_progress: snap.benchmark_progress as f64,
            overall_rank: snap.overall_rank.max(0) as u32,
            categories,
        }
    }

    fn build_card(
        state: &AppState,
        steam_id: &str,
        benchmark_id: i64,
        favorite_ids: &std::collections::HashSet<i64>,
    ) -> kovaaks_core::Result<Option<BenchmarkCard>> {
        let Some((bench, difficulty)) = state.registry.by_id(benchmark_id as u64) else {
            return Ok(None); // snapshot for a difficulty no longer in the registry
        };
        let history = state.store.history(steam_id, benchmark_id)?;
        let latest = history.last();
        let metrics = metrics_for_benchmark(&state.store, steam_id, benchmark_id)?;
        let progress = latest.map(|s| s.benchmark_progress).unwrap_or(0);
        let overall_rank = latest.map(|s| s.overall_rank).unwrap_or(0).max(0) as u32;
        // v0.2 rank engine: recompute the rank the way evxl does. Stored
        // snapshots and the engine both work in display units — no rescaling.
        let rank_tier = latest
            .and_then(|snap| {
                let api_progress = stored_to_progress(snap);
                kovaaks_core::rankcalc::compute_rank(&api_progress, bench, &difficulty)
                    .tier(&difficulty)
            })
            .or_else(|| kovaaks_core::rank_from_index(overall_rank, &difficulty));
        let ladder = overall_ladder(latest);
        let (next_name, next_delta) = next_rank_from_ladder(progress, &ladder, &difficulty);
        Ok(Some(BenchmarkCard {
            benchmark_id,
            benchmark_name: bench.name.clone(),
            abbreviation: bench.abbreviation.clone(),
            difficulty_name: difficulty.name.clone(),
            rank: rank_tier,
            benchmark_progress: progress,
            next_rank_name: next_name,
            next_rank_delta: next_delta,
            avg_score: metrics.avg_score,
            high_score: metrics.high_score,
            avg_improvement_pct: metrics.avg_improvement_pct,
            high_improvement_pct: metrics.high_improvement_pct,
            samples: metrics.samples,
            last_synced: latest.map(|s| s.captured_at.to_rfc3339()),
            is_favorite: favorite_ids.contains(&benchmark_id),
            snapshot_history: history
                .iter()
                .map(|s| SnapshotPoint {
                    captured_at: s.captured_at.to_rfc3339(),
                    benchmark_progress: s.benchmark_progress,
                })
                .collect(),
        }))
    }

    /// Engine-computed rank diffs between the two newest snapshots of every
    /// benchmark, for post-sync toast surfacing. Read-only.
    #[tauri::command]
    pub fn rank_changes(state: State<'_, AppState>) -> Result<Vec<RankChange>, String> {
        let steam_id = state
            .profile()
            .map_err(|e| e.to_string())?
            .map(|p| p.steam_id)
            .ok_or("no profile connected")?;
        let state = state.inner();
        kovaaks_core::rankdiff::compute_rank_changes(&state.store, state.registry, &steam_id)
            .map_err(|e| e.to_string())
    }

    /// Overview grid: one card per played benchmark, sorted by benchmark name.
    /// Sync commands run on a thread-pool thread (not the main thread) so a
    /// wide card grid never blocks the webview event loop.
    #[tauri::command]
    pub async fn get_overview(state: State<'_, AppState>) -> Result<Vec<BenchmarkCard>, String> {
        let state = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let steam_id = state
                .profile()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "no profile connected".to_string())?
                .steam_id;
            let favorite_ids: std::collections::HashSet<i64> = state
                .store
                .favorites(&steam_id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .collect();
            let mut cards = Vec::new();
            for benchmark_id in state
                .store
                .played_benchmarks(&steam_id)
                .map_err(|e| e.to_string())?
            {
                if let Some(card) = build_card(&state, &steam_id, benchmark_id, &favorite_ids)
                    .map_err(|e| e.to_string())?
                {
                    cards.push(card);
                }
            }
            // Favorites pinned on top (pin order), then alphabetical.
            cards.sort_by(|a, b| {
                b.is_favorite
                    .cmp(&a.is_favorite)
                    .then_with(|| a.benchmark_name.cmp(&b.benchmark_name))
            });
            Ok(cards)
        })
        .await
        .map_err(|e| format!("overview join error: {e}"))?
    }

    /// Full detail for one benchmark: card + snapshot history + CSV plays +
    /// scenario tiers + per-category progress from the newest snapshot.
    #[tauri::command]
    pub async fn get_benchmark_detail(
        state: State<'_, AppState>,
        benchmark_id: i64,
    ) -> Result<BenchmarkDetail, String> {
        let state = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let steam_id = state
                .profile()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "no profile connected".to_string())?
                .steam_id;
            let card = build_card(
                &state,
                &steam_id,
                benchmark_id,
                &std::collections::HashSet::new(),
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("unknown benchmark id {benchmark_id}"))?;
            let (_, difficulty) = state
                .registry
                .by_id(benchmark_id as u64)
                .ok_or_else(|| format!("unknown benchmark id {benchmark_id}"))?;

            let history = state
                .store
                .history(&steam_id, benchmark_id)
                .map_err(|e| e.to_string())?;
            let snapshot_history: Vec<SnapshotPoint> = history
                .iter()
                .map(|s| SnapshotPoint {
                    captured_at: s.captured_at.to_rfc3339(),
                    benchmark_progress: s.benchmark_progress,
                })
                .collect();

            let latest = history.last();
            let scenario_ranks: Vec<ScenarioRank> = latest
                .map(|s| s.scenarios.clone())
                .unwrap_or_default()
                .into_iter()
                .map(|row| ScenarioRank {
                    tier: kovaaks_core::rank_from_index(
                        row.scenario_rank.max(0) as u32,
                        &difficulty,
                    ),
                    scenario: row.scenario,
                    score: row.score,
                    leaderboard_rank: row.leaderboard_rank,
                    scenario_rank: row.scenario_rank,
                    rank_maxes: row.rank_maxes.clone(),
                })
                .collect();

            // CSV plays for every scenario of the newest snapshot, merged +
            // sorted; each point keeps its scenario so the UI can scope the
            // underlay to the selected scenario. Scenarios missing from the
            // snapshot entirely still need their plays, so scan ALL local
            // plays of scenarios seen in any snapshot of this benchmark.
            // Kept as typed tuples through both consumers below; serialized
            // to `PlayPoint` rfc3339 strings only at the DTO boundary.
            let mut benchmark_scenarios: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for snap in &history {
                for row in &snap.scenarios {
                    benchmark_scenarios.insert(row.scenario.clone());
                }
            }
            let mut plays: Vec<(String, chrono::DateTime<chrono::Utc>, f64)> = Vec::new();
            for scenario in &benchmark_scenarios {
                for play in state
                    .store
                    .plays_history(&steam_id, scenario)
                    .map_err(|e| e.to_string())?
                {
                    plays.push((scenario.clone(), play.played_at, play.score));
                }
            }
            plays.sort_by_key(|p| p.1);
            let play_points: Vec<PlayPoint> = plays
                .iter()
                .map(|(scenario, played_at, score)| PlayPoint {
                    scenario: scenario.clone(),
                    played_at: played_at.to_rfc3339(),
                    score: *score,
                })
                .collect();

            // Per-category progress: sum of that category's scenario scores in the
            // newest snapshot; tier from the API's own category_rank index.
            let mut per_category: BTreeMap<String, Vec<&kovaaks_core::store::StoredScenario>> =
                BTreeMap::new();
            if let Some(snapshot) = latest {
                for row in &snapshot.scenarios {
                    per_category
                        .entry(row.category.clone())
                        .or_default()
                        .push(row);
                }
            }
            let categories: Vec<CategoryCard> = per_category
                .into_iter()
                .map(|(name, rows)| {
                    let progress: i64 = rows.iter().map(|r| r.score).sum();
                    let category_rank = rows
                        .iter()
                        .map(|r| r.category_rank.max(0) as u32)
                        .max()
                        .unwrap_or(0);
                    CategoryCard {
                        name,
                        progress,
                        rank_tier: kovaaks_core::rank_from_index(category_rank, &difficulty),
                    }
                })
                .collect();

            // Per-scenario chart series: synced snapshot points, falling back
            // to local CSV plays for scenarios without a synced score (see
            // kovaaks_core::metrics::build_scenario_history). `plays` already
            // carries typed timestamps — no string round-trip.
            let scenario_history: Vec<ScenarioHistorySeries> =
                kovaaks_core::metrics::build_scenario_history(&history, &plays)
                    .into_iter()
                    .map(|s| ScenarioHistorySeries {
                        scenario: s.scenario,
                        category: s.category,
                        source: s.source.as_str().to_string(),
                        points: s
                            .points
                            .into_iter()
                            .map(|(captured_at, score)| ScenarioHistoryPoint {
                                captured_at: captured_at.to_rfc3339(),
                                score,
                            })
                            .collect(),
                    })
                    .collect();

            Ok(BenchmarkDetail {
                card,
                snapshot_history,
                plays: play_points,
                scenario_ranks,
                categories,
                scenario_history,
                rank_tiers: difficulty.rank_colors.clone(),
                scenario_metrics: {
                    let mut map = std::collections::BTreeMap::new();
                    let latest_rows = latest
                        .map(|snap| snap.scenarios.clone())
                        .unwrap_or_default();
                    for s in &latest_rows {
                        map.insert(
                            s.scenario.clone(),
                            metrics_for_scenario_combined(
                                &state.store,
                                &steam_id,
                                benchmark_id,
                                &s.scenario,
                            )
                            .unwrap_or_default(),
                        );
                    }
                    map
                },
            })
        })
        .await
        .map_err(|e| format!("detail join error: {e}"))?
    }

    /// CSV ingest counters from the last scan + last sync timestamp.
    #[tauri::command]
    pub fn ingest_status(state: State<'_, AppState>) -> Result<IngestStatus, String> {
        Ok(ingest_status_from(&state.store))
    }

    /// Re-scan the local KovaaK's stats CSVs (no network). Forward-only like
    /// the sync-time scan; returns the refreshed ingest counters.
    #[tauri::command]
    pub async fn refresh_local(state: State<'_, AppState>) -> Result<IngestStatus, String> {
        let state = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            // Prefer the scan's own counts when it ran; fall back to the
            // stored meta (last scan's counters) when there was no profile.
            let status = match run_csv_scan(&state.store) {
                Some((seen, inserted)) => IngestStatus {
                    csv_seen: seen as u64,
                    csv_inserted: inserted as u64,
                    last_synced_at: state.store.get_meta(LAST_SYNCED_KEY).ok().flatten(),
                },
                None => ingest_status_from(&state.store),
            };
            Ok(status)
        })
        .await
        .map_err(|e| format!("refresh join error: {e}"))?
    }

    /// Current app settings.
    #[tauri::command]
    pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
        Ok(state.load_settings())
    }

    /// Toggle a benchmark's favorite pin. Returns the new state (true = pinned).
    #[tauri::command]
    pub fn toggle_favorite(state: State<'_, AppState>, benchmark_id: i64) -> Result<bool, String> {
        let steam_id = state
            .profile()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no profile connected".to_string())?
            .steam_id;
        let favorites = state
            .store
            .favorites(&steam_id)
            .map_err(|e| e.to_string())?;
        if favorites.contains(&benchmark_id) {
            state
                .store
                .remove_favorite(&steam_id, benchmark_id)
                .map_err(|e| e.to_string())?;
            Ok(false)
        } else {
            state
                .store
                .add_favorite(&steam_id, benchmark_id)
                .map_err(|e| e.to_string())?;
            Ok(true)
        }
    }

    /// Persist app settings (stats dir override, sync interval).
    #[tauri::command]
    pub fn set_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<(), String> {
        let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
        state
            .store
            .set_meta(SETTINGS_KEY, &json)
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// Tauri entry point: state, setup hooks, command registration.
pub fn run() {
    let store = Store::open(&db_path()).expect("open sqlite store");
    csv_ingest::ensure_cutoff(&store).expect("seed first-run csv cutoff");
    let registry: &'static Registry = Box::leak(Box::new(Registry));
    let scan_store = store.clone();
    tauri::Builder::default()
        .setup(move |_app| {
            // First-run CSV scan in the background (tolerates a missing
            // KovaaK's install; no-ops until a profile is connected).
            tauri::async_runtime::spawn_blocking(move || run_csv_scan(&scan_store));
            Ok(())
        })
        .manage(AppState {
            store,
            registry,
            evxl: EvxlClient::new().expect("evxl http client"),
        })
        .invoke_handler(tauri::generate_handler![
            commands::resolve_profile,
            commands::get_profile,
            commands::sync_now,
            commands::rank_changes,
            commands::get_overview,
            commands::get_benchmark_detail,
            commands::ingest_status,
            commands::refresh_local,
            commands::get_settings,
            commands::set_settings,
            commands::toggle_favorite,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REGRESSION (skeleton-forever bug): the frontend reads snake_case fields
    /// (benchmark_progress, next_rank_delta, …). camelCase wire output made
    /// every card render throw `undefined.toLocaleString` and the overview
    /// froze on skeletons. If this test fails, the DTO serde casing drifted
    /// from ui/src/lib/api.ts again.
    #[test]
    fn dto_wire_format_is_snake_case() {
        let card = BenchmarkCard {
            benchmark_id: 459,
            benchmark_name: "Voltaic S5".into(),
            abbreviation: "VT".into(),
            difficulty_name: "Novice".into(),
            rank: Some(RankTier {
                name: "Gold".into(),
                color: "#CAB148".into(),
            }),
            benchmark_progress: 180000,
            next_rank_name: None,
            next_rank_delta: None,
            avg_score: 1.0,
            high_score: 1.0,
            avg_improvement_pct: None,
            high_improvement_pct: None,
            samples: 1,
            last_synced: None,
            is_favorite: false,
            snapshot_history: vec![],
        };
        let json = serde_json::to_string(&card).unwrap();
        for key in [
            "\"benchmark_id\"",
            "\"benchmark_name\"",
            "\"difficulty_name\"",
            "\"benchmark_progress\"",
            "\"next_rank_name\"",
            "\"next_rank_delta\"",
            "\"avg_score\"",
            "\"high_score\"",
            "\"avg_improvement_pct\"",
            "\"high_improvement_pct\"",
            "\"last_synced\"",
            "\"snapshot_history\"",
        ] {
            assert!(json.contains(key), "missing {key} in {json}");
        }
        assert!(
            !json.contains("benchmarkId"),
            "camelCase leaked into wire format: {json}"
        );

        let detail = BenchmarkDetail {
            card: card.clone(),
            snapshot_history: vec![],
            plays: vec![PlayPoint {
                scenario: "VT Pasu Novice S5".into(),
                played_at: "2026-09-02T22:00:00Z".into(),
                score: 1281.61,
            }],
            scenario_ranks: vec![ScenarioRank {
                scenario: "VT Pasu Novice S5".into(),
                score: 128161,
                leaderboard_rank: 169,
                tier: None,
                scenario_rank: 4,
                rank_maxes: vec![555.0, 660.0, 745.0, 800.0],
            }],
            categories: vec![CategoryCard {
                name: "Clicking".into(),
                progress: 60000,
                rank_tier: None,
            }],
            scenario_history: vec![ScenarioHistorySeries {
                scenario: "VT Pasu Novice S5".into(),
                category: "Clicking".into(),
                source: "snapshot".into(),
                points: vec![ScenarioHistoryPoint {
                    captured_at: "2026-09-02T22:00:00Z".into(),
                    score: 1282,
                }],
            }],
            rank_tiers: vec![
                RankTier {
                    name: "Iron".into(),
                    color: "#999999".into(),
                },
                RankTier {
                    name: "Bronze".into(),
                    color: "#FF9900".into(),
                },
                RankTier {
                    name: "Silver".into(),
                    color: "#CBD9E6".into(),
                },
                RankTier {
                    name: "Gold".into(),
                    color: "#CAB148".into(),
                },
            ],
            scenario_metrics: std::collections::BTreeMap::from([(
                "VT Pasu Novice S5".to_string(),
                kovaaks_core::Metrics {
                    avg_score: 1280.0,
                    high_score: 1282.0,
                    avg_improvement_pct: Some(5.0),
                    high_improvement_pct: Some(2.0),
                    samples: 2,
                },
            )]),
        };
        let json = serde_json::to_string(&detail).unwrap();
        for key in [
            "\"scenario_ranks\"",
            "\"leaderboard_rank\"",
            "\"rank_tier\"",
            "\"snapshot_history\"",
        ] {
            assert!(json.contains(key), "missing {key} in {json}");
        }
        assert!(
            !json.contains("leaderboardRank"),
            "camelCase leaked: {json}"
        );
    }

    #[test]
    fn settings_roundtrip_with_defaults() {
        let json = serde_json::to_string(&AppSettings::default()).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AppSettings::default());
        assert_eq!(back.sync_interval_hours, 6);
        // Corrupt blob falls back to defaults, never panics.
        let corrupt: Option<AppSettings> = serde_json::from_str("{oops").ok();
        assert_eq!(corrupt, None);
    }

    #[test]
    fn ladder_from_rows_sorts_and_dedups() {
        assert_eq!(
            ladder_from_rows(&[1000.0, 500.0, 1000.0, 2000.0]),
            vec![500, 1000, 2000]
        );
        assert!(ladder_from_rows(&[]).is_empty());
    }

    #[test]
    fn next_rank_picks_threshold_strictly_above_progress() {
        let difficulty = Difficulty {
            name: "Test".into(),
            kovaaks_benchmark_id: 1,
            sharecode: String::new(),
            rank_colors: vec![
                RankTier {
                    name: "Bronze".into(),
                    color: "#000".into(),
                },
                RankTier {
                    name: "Silver".into(),
                    color: "#111".into(),
                },
                RankTier {
                    name: "Gold".into(),
                    color: "#222".into(),
                },
            ],
            categories: Vec::new(),
        };
        let ladder = [500, 1000, 2000];
        let (name, delta) = next_rank_from_ladder(400, &ladder, &difficulty);
        assert_eq!(name.as_deref(), Some("Bronze"));
        assert_eq!(delta, Some(100));
        let (name, delta) = next_rank_from_ladder(500, &ladder, &difficulty);
        assert_eq!(name.as_deref(), Some("Silver"));
        assert_eq!(delta, Some(500));
        let (name, delta) = next_rank_from_ladder(2000, &ladder, &difficulty);
        assert_eq!(name, None);
        assert_eq!(delta, None);
        let (name, delta) = next_rank_from_ladder(0, &[], &difficulty);
        assert_eq!(name, None);
        assert_eq!(delta, None);
    }
}
