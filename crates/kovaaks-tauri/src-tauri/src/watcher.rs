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

/// Whether the app is shutting down (thread exits with the process anyway;
/// this only avoids a pointless final scan on the teardown path).
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

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
                    let _ = app.emit(
                        "local-plays-updated",
                        serde_json::json!({ "seen": seen, "inserted": inserted }),
                    );
                }
            }
            last_sig = sig;
        }
    });
}
