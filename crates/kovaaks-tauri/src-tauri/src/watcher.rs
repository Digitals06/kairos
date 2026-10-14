//! Live CSV watcher: keeps charts/lists fresh while the app is open.
//!
//! A detached std thread polls the stats dir every `POLL_SECS` and compares a
//! cheap signature (file count + newest mtime). On change, it runs the same
//! forward-only `run_csv_scan` the Sync Now path uses (ingest on quiesce —
//! KovaaK's writes incrementally mid-session, the signature lag is the
//! debounce) and emits a `local-plays-updated` Tauri event with the report.
//!
//! Strictly local: no network, ranks still change only on Sync Now.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use tauri::Emitter;

use crate::{run_csv_scan, AppState};

/// Poll cadence. The mtime/count signature doubles as the debounce: KovaaK's
/// writes land between polls, so ingestion happens on quiesce.
const POLL_SECS: u64 = 3;

/// Meta key storing the last ingested plays' newest timestamp (unix secs).
const SESSION_LAST_KEY: &str = "session_last_play_ts";

/// Whether the app is shutting down (thread exits with the process anyway;
/// this only avoids a pointless final scan on the teardown path).
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// A gap larger than this between consecutive plays starts a new session
/// (KovaaK's writes a play per scenario run; live sessions write every
/// minute at most, so 20+ minutes means the player left and came back).
const SESSION_GAP_SECS: i64 = 20 * 60;

/// A new session starts when the gap between the previous last play and a
/// newly ingested play exceeds the session gap.
fn session_boundary(prev_last_ts: i64, new_ts: i64) -> bool {
    new_ts - prev_last_ts >= SESSION_GAP_SECS
}

/// Signature of the stats dir: (file count, newest mtime). `None` when the
/// dir is unreadable (missing/unset settings) — an absent dir and an error
/// both hash the same so we don't rescan a missing dir every tick.
fn dir_signature(dir: &PathBuf) -> Option<(usize, SystemTime)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut count = 0usize;
    let mut newest = SystemTime::UNIX_EPOCH;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        count += 1;
        if let Ok(mtime) = meta.modified() {
            if mtime > newest {
                newest = mtime;
            }
        }
    }
    Some((count, newest))
}

/// Spawn the background watcher. Called once from `run()` after state setup.
pub fn spawn(app: tauri::AppHandle, state: AppState) {
    std::thread::spawn(move || {
        let mut last_sig: Option<(usize, SystemTime)> = None;
        let mut last_dir: Option<PathBuf> = None;
        loop {
            if SHUTDOWN.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_secs(POLL_SECS));

            // Re-resolve the dir every tick so a settings change takes
            // effect without an app restart.
            let dir = state.stats_dir();
            let sig = dir_signature(&dir);

            // Reset the baseline when the dir itself changed (settings edit):
            // the next tick then compares against the new dir's signature.
            if last_dir.as_ref() != Some(&dir) {
                last_dir = Some(dir);
                last_sig = sig;
                continue;
            }

            if sig.is_some() && sig != last_sig {
                let (seen, inserted) = match run_csv_scan(&state.store) {
                    Some(pair) => pair,
                    None => {
                        // No steam_id connected or cutoff failure: keep the
                        // baseline so we don't rescan until something changes.
                        last_sig = sig;
                        continue;
                    }
                };
                if inserted > 0 {
                    // Post-session summary: a play above every earlier score
                    // for its scenario is a PB. A >20 min idle gap before
                    // this batch marks the end of the previous session; the
                    // summary covers the just-finished one.
                    let summary =
                        state
                            .profile()
                            .ok()
                            .flatten()
                            .map(|p| p.steam_id)
                            .and_then(|sid| {
                                let prev_marker = state
                                    .store
                                    .get_meta(SESSION_LAST_KEY)
                                    .ok()
                                    .flatten()
                                    .and_then(|v| v.parse::<i64>().ok());
                                let plays = state.store.latest_plays(&sid, inserted).ok()?;
                                let new_last = plays.first()?.played_at;
                                let is_new_session = match prev_marker {
                                    Some(ts) => session_boundary(ts, new_last.timestamp()),
                                    None => false, // first pass ever: seed, no toast
                                };
                                let _ = state
                                    .store
                                    .set_meta(SESSION_LAST_KEY, &new_last.timestamp().to_string());
                                if !is_new_session {
                                    return None;
                                }
                                // The just-finished session's plays: everything
                                // ingested after (prev marker - gap).
                                let window_start = chrono::DateTime::<chrono::Utc>::from_timestamp(
                                    prev_marker.unwrap_or(new_last.timestamp()),
                                    0,
                                )
                                .unwrap_or(new_last)
                                    - chrono::Duration::seconds(SESSION_GAP_SECS);
                                let mut plays_in_session = 0u32;
                                let mut pbs = 0u32;
                                let mut best_delta: Option<f64> = None;
                                let mut best_scenario = String::new();
                                for play in &plays {
                                    if play.played_at <= window_start {
                                        break; // newest-first order
                                    }
                                    plays_in_session += 1;
                                    let hist =
                                        state.store.plays_history(&sid, &play.scenario).ok()?;
                                    let prior_best = hist
                                        .iter()
                                        .take_while(|h| h.played_at < play.played_at)
                                        .map(|h| h.score)
                                        .fold(-1.0f64, f64::max);
                                    if play.score > prior_best {
                                        let delta = play.score - prior_best.max(0.0);
                                        pbs += 1;
                                        if best_delta.is_none_or(|d| delta > d) {
                                            best_delta = Some(delta);
                                            best_scenario = play.scenario.clone();
                                        }
                                    }
                                }
                                Some((plays_in_session, pbs, best_delta, best_scenario))
                            });
                    let mut payload = serde_json::json!({ "seen": seen, "inserted": inserted });
                    if let Some((plays_count, pbs, best_delta, best_scenario)) = summary {
                        payload["session"] = serde_json::json!({
                            "plays": plays_count,
                            "pbs": pbs,
                            "best_delta": best_delta,
                            "best_scenario": best_scenario,
                        });
                    }
                    let _ = app.emit("local-plays-updated", payload);
                }
            }
            last_sig = sig;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 40-min gap between two plays starts a new session; 5-min does not.
    #[test]
    fn session_boundary_flags_half_hour_gaps() {
        assert!(session_boundary(1_000, 1_000 + 40 * 60));
        assert!(session_boundary(1_000, 1_000 + 30 * 60));
        assert!(!session_boundary(1_000, 1_000 + 5 * 60));
        assert!(!session_boundary(5_000, 4_900)); // backwards: no boundary
    }
}
