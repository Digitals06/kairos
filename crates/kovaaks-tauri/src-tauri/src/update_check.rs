//! In-app update check (v0.6.0): store-backed cache, status DTO and commands.
//!
//! The network fetch lives in `kovaaks_core::update_check` so it shares the
//! app's reqwest client (UA/timeout/TLS); this module owns persistence, the
//! launch-time timer and the two UI-facing commands. Read-only: no download,
//! no install, failures are logged at most — never a dialog.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use ts_rs::TS;

use kovaaks_core::Store;

use crate::AppState;

/// Meta key: RFC3339 timestamp of the last successful GitHub check.
pub const UPDATE_CHECKED_AT_KEY: &str = "update_check_at";
/// Meta key: newest release version (without the tag's `v` prefix).
pub const UPDATE_LATEST_KEY: &str = "update_latest_version";
/// GitHub web tag page prefix for a specific release.
const RELEASE_TAG_URL: &str = "https://github.com/Digitals06/kairos/releases/tag/v";
/// Deferred so launch traffic (CSV scan, registry check) paints first.
const DEFERRED_CHECK_DELAY: Duration = Duration::from_secs(10);

/// Update chip state the UI reads at boot and after a manual check.
#[derive(TS)]
#[ts(export)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct UpdateStatusDto {
    /// Version of the running app (Tauri package info, not hardcoded).
    pub current_version: String,
    /// Newest release tag, when a check has succeeded at least once.
    pub latest_version: Option<String>,
    /// True only when `latest_version` is strictly newer than `current_version`.
    pub update_available: bool,
    /// RFC3339 timestamp of the check that produced `latest_version`.
    pub checked_at: Option<String>,
}

/// The running app's version straight from Tauri's package metadata.
pub fn current_version(app: &tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Cached status: no network. Missing/corrupt meta degrades to "no update".
pub fn read_status(store: &Store, current: &str) -> UpdateStatusDto {
    let latest = store
        .get_meta(UPDATE_LATEST_KEY)
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty());
    let checked_at = store.get_meta(UPDATE_CHECKED_AT_KEY).ok().flatten();
    let update_available = latest
        .as_deref()
        .is_some_and(|latest| kovaaks_core::update_check::newer_than(latest, current));
    UpdateStatusDto {
        current_version: current.to_string(),
        latest_version: latest,
        update_available,
        checked_at,
    }
}

/// Fetch once and persist on success. Failures are logged and ignored.
async fn refresh(store: &Store) {
    match kovaaks_core::update_check::fetch_latest_version().await {
        Ok(Some(version)) => {
            let _ = store.set_meta(UPDATE_LATEST_KEY, &version);
            let _ = store.set_meta(UPDATE_CHECKED_AT_KEY, &chrono::Utc::now().to_rfc3339());
        }
        Ok(None) => eprintln!("update check: latest release tag missing or malformed"),
        Err(e) => eprintln!("update check skipped: {e}"),
    }
}

/// Launch-time check: deferred ~10s, gated by the opt-out setting and the
/// 24h cache. Runs on the Tauri runtime so setup returns immediately.
pub(crate) fn spawn_launch_check(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(DEFERRED_CHECK_DELAY).await;
        let (enabled, store) = {
            let state = app.state::<AppState>();
            (
                state.load_settings().update_check_enabled,
                state.store.clone(),
            )
        };
        if !enabled {
            return;
        }
        let cached_at = store.get_meta(UPDATE_CHECKED_AT_KEY).ok().flatten();
        let fresh = cached_at
            .as_deref()
            .is_some_and(|at| kovaaks_core::update_check::cache_is_fresh(at, chrono::Utc::now()));
        if fresh {
            return;
        }
        refresh(&store).await;
    });
}

/// Cached update state (no network). The UI reads this at boot.
#[tauri::command]
pub fn update_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateStatusDto, String> {
    Ok(read_status(&state.store, &current_version(&app)))
}

/// Explicit gear-menu action: bypass the 24h cache and ask GitHub now.
/// A failed check leaves the last known state in place (quiet).
#[tauri::command]
pub async fn check_update_now(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateStatusDto, String> {
    let current = current_version(&app);
    let store = state.store.clone();
    refresh(&store).await;
    Ok(read_status(&store, &current))
}

/// Open the newest release page in the system browser. Backend-side so the
/// webview needs no opener capability (same pattern as the tray deep links).
#[tauri::command]
pub fn open_release_page(state: State<'_, AppState>) {
    let tag = state
        .store
        .get_meta(UPDATE_LATEST_KEY)
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty());
    let url = match tag {
        Some(version) => format!("{RELEASE_TAG_URL}{version}"),
        None => kovaaks_core::update_check::RELEASES_PAGE.to_string(),
    };
    crate::open_url(&url);
}
