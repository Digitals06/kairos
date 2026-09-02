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
    csv_ingest, metrics_for_benchmark, rank_for,
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
const DEFAULT_STATS_DIR: &str = "C:\\Program Files (x86)\\Steam\\steamapps\\common\\FPSAimTrainer\\FPSAimTrainer\\stats";
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
    /// Full snapshot history so the UI can draw sparklines without an N+1 of
    /// per-card detail calls (those starve the main thread on 70-card grids).
    pub snapshot_history: Vec<SnapshotPoint>,
}

/// One scenario row in the benchmark detail view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScenarioRank {
    pub scenario: String,
    pub score: i64,
    pub leaderboard_rank: i64,
    pub tier: Option<RankTier>,
}

/// One category row in the benchmark detail view.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PlayPoint {
    pub played_at: String,
    pub score: f64,
}

/// Full detail payload for one benchmark.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BenchmarkDetail {
    pub card: BenchmarkCard,
    pub snapshot_history: Vec<SnapshotPoint>,
    pub plays: Vec<PlayPoint>,
    pub scenario_ranks: Vec<ScenarioRank>,
    pub categories: Vec<CategoryCard>,
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
        Self { ok: r.ok, failed: r.failed, errors: r.errors }
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
        Self { stats_dir: String::new(), sync_interval_hours: 6 }
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

/// SQLite DB path: `%LOCALAPPDATA%\kovaaks-companion\store.db` (dir created).
fn db_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").map_or_else(
        |_| std::env::var("USERPROFILE")
            .map(|h| PathBuf::from(h).join("AppData").join("Local"))
            .unwrap_or_else(|_| PathBuf::from(".")),
        PathBuf::from,
    );
    let dir = base.join("kovaaks-companion");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("store.db")
}

/// CSV ingest scan (forward-only from the stored first-run cutoff). Tolerates
/// a missing KovaaK's install — scan errors land on stderr and the counters
/// stay at their previous values. Runs without a profile as a no-op; called
/// again from `resolve_profile` once a player is connected.
fn run_csv_scan(store: &Store) {
    let steam_id = match store.get_meta("steam_id") {
        Ok(Some(id)) if !id.is_empty() => id,
        _ => return,
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
        Err(_) => return,
    };
    let report = csv_ingest::scan_dir(&dir, cutoff, store, &steam_id);
    let _ = store.set_meta(INGEST_SEEN_KEY, &report.seen.to_string());
    let _ = store.set_meta(INGEST_INSERTED_KEY, &report.inserted.to_string());
    for e in report.errors.iter().take(5) {
        eprintln!("csv ingest: {e}");
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
        state.store.upsert_player(&profile).map_err(|e| e.to_string())?;
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
    fn build_card(
        state: &AppState,
        steam_id: &str,
        benchmark_id: i64,
    ) -> kovaaks_core::Result<Option<BenchmarkCard>> {
        let Some((bench, difficulty)) = state.registry.by_id(benchmark_id as u64) else {
            return Ok(None); // snapshot for a difficulty no longer in the registry
        };
        let history = state.store.history(steam_id, benchmark_id)?;
        let latest = history.last();
        let metrics = metrics_for_benchmark(&state.store, steam_id, benchmark_id)?;
        let progress = latest.map(|s| s.benchmark_progress).unwrap_or(0);
        let ladder = overall_ladder(latest);
        let (next_name, next_delta) = next_rank_from_ladder(progress, &ladder, &difficulty);
        Ok(Some(BenchmarkCard {
            benchmark_id,
            benchmark_name: bench.name.clone(),
            abbreviation: bench.abbreviation.clone(),
            difficulty_name: difficulty.name.clone(),
            rank: rank_for(progress, &ladder, &difficulty),
            benchmark_progress: progress,
            next_rank_name: next_name,
            next_rank_delta: next_delta,
            avg_score: metrics.avg_score,
            high_score: metrics.high_score,
            avg_improvement_pct: metrics.avg_improvement_pct,
            high_improvement_pct: metrics.high_improvement_pct,
            samples: metrics.samples,
            last_synced: latest.map(|s| s.captured_at.to_rfc3339()),
            snapshot_history: history
                .iter()
                .map(|s| SnapshotPoint {
                    captured_at: s.captured_at.to_rfc3339(),
                    benchmark_progress: s.benchmark_progress,
                })
                .collect(),
        }))
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
            let mut cards = Vec::new();
            for benchmark_id in state
                .store
                .played_benchmarks(&steam_id)
                .map_err(|e| e.to_string())?
            {
                if let Some(card) =
                    build_card(&state, &steam_id, benchmark_id).map_err(|e| e.to_string())?
                {
                    cards.push(card);
                }
            }
            cards.sort_by(|a, b| a.benchmark_name.cmp(&b.benchmark_name));
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
            let card = build_card(&state, &steam_id, benchmark_id)
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
                    tier: kovaaks_core::scenario_rank_tier(
                        row.scenario_rank.max(0) as usize,
                        &difficulty,
                    ),
                    scenario: row.scenario,
                    score: row.score,
                    leaderboard_rank: row.leaderboard_rank,
                })
                .collect();

            // CSV plays for every scenario of the newest snapshot, merged + sorted.
            let mut plays: Vec<(chrono::DateTime<chrono::Utc>, f64)> = Vec::new();
            if let Some(snapshot) = latest {
                let mut scenarios: Vec<&str> = snapshot
                    .scenarios
                    .iter()
                    .map(|s| s.scenario.as_str())
                    .collect();
                scenarios.sort_unstable();
                scenarios.dedup();
                for scenario in scenarios {
                    for play in state
                        .store
                        .plays_history(&steam_id, scenario)
                        .map_err(|e| e.to_string())?
                    {
                        plays.push((play.played_at, play.score));
                    }
                }
            }
            plays.sort_by(|a, b| a.0.cmp(&b.0));
            let plays: Vec<PlayPoint> = plays
                .into_iter()
                .map(|(played_at, score)| PlayPoint {
                    played_at: played_at.to_rfc3339(),
                    score,
                })
                .collect();

            // Per-category progress: sum of that category's scenario scores in the
            // newest snapshot, tiered against the category's own ladder.
            let mut per_category: BTreeMap<String, Vec<&kovaaks_core::store::StoredScenario>> =
                BTreeMap::new();
            if let Some(snapshot) = latest {
                for row in &snapshot.scenarios {
                    per_category.entry(row.category.clone()).or_default().push(row);
                }
            }
            let categories: Vec<CategoryCard> = per_category
                .into_iter()
                .map(|(name, rows)| {
                    let progress: i64 = rows.iter().map(|r| r.score).sum();
                    let ladder = rows
                        .iter()
                        .find(|r| !r.rank_maxes.is_empty())
                        .map(|r| ladder_from_rows(&r.rank_maxes))
                        .unwrap_or_default();
                    CategoryCard {
                        name,
                        progress,
                        rank_tier: rank_for(progress, &ladder, &difficulty),
                    }
                })
                .collect();

            Ok(BenchmarkDetail { card, snapshot_history, plays, scenario_ranks, categories })
        })
        .await
        .map_err(|e| format!("detail join error: {e}"))?
    }

    /// CSV ingest counters from the last scan + last sync timestamp.
    #[tauri::command]
    pub fn ingest_status(state: State<'_, AppState>) -> Result<IngestStatus, String> {
        let meta_num = |key: &str| -> u64 {
            state
                .store
                .get_meta(key)
                .ok()
                .flatten()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        Ok(IngestStatus {
            csv_seen: meta_num(INGEST_SEEN_KEY),
            csv_inserted: meta_num(INGEST_INSERTED_KEY),
            last_synced_at: state.store.get_meta(LAST_SYNCED_KEY).ok().flatten(),
        })
    }

    /// Current app settings.
    #[tauri::command]
    pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
        Ok(state.load_settings())
    }

    /// Persist app settings (stats dir override, sync interval).
    #[tauri::command]
    pub fn set_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<(), String> {
        let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
        state.store.set_meta(SETTINGS_KEY, &json).map_err(|e| e.to_string())
    }
}


// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

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
            rank: Some(RankTier { name: "Gold".into(), color: "#CAB148".into() }),
            benchmark_progress: 180000,
            next_rank_name: None,
            next_rank_delta: None,
            avg_score: 1.0,
            high_score: 1.0,
            avg_improvement_pct: None,
            high_improvement_pct: None,
            samples: 1,
            last_synced: None,
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
        assert!(!json.contains("benchmarkId"), "camelCase leaked into wire format: {json}");

        let detail = BenchmarkDetail {
            card: card.clone(),
            snapshot_history: vec![],
            plays: vec![],
            scenario_ranks: vec![ScenarioRank {
                scenario: "VT Pasu Novice S5".into(),
                score: 128161,
                leaderboard_rank: 169,
                tier: None,
            }],
            categories: vec![CategoryCard {
                name: "Clicking".into(),
                progress: 60000,
                rank_tier: None,
            }],
        };
        let json = serde_json::to_string(&detail).unwrap();
        for key in ["\"scenario_ranks\"", "\"leaderboard_rank\"", "\"rank_tier\"", "\"snapshot_history\""] {
            assert!(json.contains(key), "missing {key} in {json}");
        }
        assert!(!json.contains("leaderboardRank"), "camelCase leaked: {json}");
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
        assert_eq!(ladder_from_rows(&[1000.0, 500.0, 1000.0, 2000.0]), vec![500, 1000, 2000]);
        assert!(ladder_from_rows(&[]).is_empty());
    }

    #[test]
    fn next_rank_picks_threshold_strictly_above_progress() {
        let difficulty = Difficulty {
            name: "Test".into(),
            kovaaks_benchmark_id: 1,
            sharecode: String::new(),
            rank_colors: vec![
                RankTier { name: "Bronze".into(), color: "#000".into() },
                RankTier { name: "Silver".into(), color: "#111".into() },
                RankTier { name: "Gold".into(), color: "#222".into() },
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

/// Tauri entry point: state, setup hooks, command registration.
pub fn run() {
    let store = Store::open(&db_path()).expect("open sqlite store");
    csv_ingest::ensure_cutoff(&store).expect("seed first-run csv cutoff");
    let registry: &'static Registry = Box::leak(Box::new(Registry::default()));
    let scan_store = store.clone();
    tauri::Builder::default()
        .setup(move |_app| {
            // First-run CSV scan in the background (tolerates a missing
            // KovaaK's install; no-ops until a profile is connected).
            tauri::async_runtime::spawn_blocking(move || run_csv_scan(&scan_store));
            Ok(())
        })
        .manage(AppState { store, registry, evxl: EvxlClient::new().expect("evxl http client") })
        .invoke_handler(tauri::generate_handler![
            commands::resolve_profile,
            commands::get_profile,
            commands::sync_now,
            commands::get_overview,
            commands::get_benchmark_detail,
            commands::ingest_status,
            commands::get_settings,
            commands::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
