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

use kovaaks_core::store::{goal_crossed, ScenarioGoal};
use kovaaks_core::PlayRecord;

use crate::{run_csv_scan, run_smart_sync, AppState};

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

/// Session gap in seconds, overridable via `KAIROS_SESSION_GAP_SECS` so a
/// live smoke can use a short gap. Unset/invalid/non-positive falls back to
/// the 20-minute constant.
fn session_gap_secs() -> i64 {
    std::env::var("KAIROS_SESSION_GAP_SECS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(SESSION_GAP_SECS)
}

/// A new session starts when the gap between the previous last play and a
/// newly ingested play exceeds the session gap.
fn session_boundary(prev_last_ts: i64, new_ts: i64) -> bool {
    new_ts - prev_last_ts >= SESSION_GAP_SECS
}

/// Quiescence decision: a session is active and its newest ingested play is
/// at least `gap` seconds old. `None`/inactive never fires; a backwards clock
/// (now < last play) never fires. Pure — the once-per-session guard lives in
/// the poll loop (it clears `session_active` after firing).
fn session_closed(last_play_ts: Option<i64>, now_ts: i64, gap: i64, session_active: bool) -> bool {
    if !session_active {
        return false;
    }
    let Some(last) = last_play_ts else {
        return false;
    };
    now_ts >= last.saturating_add(gap)
}

/// One personal goal crossed by an ingest batch (v0.6.0).
#[derive(Debug, Clone, PartialEq)]
struct GoalCrossing {
    scenario: String,
    score: f64,
    target: f64,
}

/// Which goals this batch newly crossed. A crossing is a batch high that meets
/// the target while the prior best (snapshot high + older plays) did not, so an
/// already-reached goal never re-toasts. Pure; `prior_best` resolves the score
/// known before the batch's earliest play for that scenario.
fn batch_goal_crossings(
    batch: &[PlayRecord],
    goals: &[ScenarioGoal],
    prior_best: impl Fn(&str, chrono::DateTime<chrono::Utc>) -> f64,
) -> Vec<GoalCrossing> {
    // Per scenario: earliest batch play (the "before" cut) + batch high.
    let mut per_scenario: std::collections::BTreeMap<&str, (chrono::DateTime<chrono::Utc>, f64)> =
        std::collections::BTreeMap::new();
    for play in batch {
        let entry = per_scenario
            .entry(play.scenario.as_str())
            .or_insert((play.played_at, play.score));
        if play.played_at < entry.0 {
            entry.0 = play.played_at;
        }
        if play.score > entry.1 {
            entry.1 = play.score;
        }
    }
    goals
        .iter()
        .filter_map(|goal| {
            let (earliest, high) = *per_scenario.get(goal.scenario.as_str())?;
            if goal_crossed(high, goal.target)
                && !goal_crossed(prior_best(&goal.scenario, earliest), goal.target)
            {
                Some(GoalCrossing {
                    scenario: goal.scenario.clone(),
                    score: high,
                    target: goal.target,
                })
            } else {
                None
            }
        })
        .collect()
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
        // Quiet auto-sync state (in-thread; seeded from the meta marker so a
        // previous run's plays don't count, but the newest timestamp is
        // known). `session_active` = plays ingested this run and not yet
        // auto-synced; cleared after the quiet sync so it fires once per
        // session, re-armed by the next ingested play.
        let mut last_play_ts: Option<i64> = state
            .store
            .get_meta(SESSION_LAST_KEY)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok());
        let mut session_active = false;
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
                    // Newest play timestamp of this batch; also arms the
                    // quiet auto-sync once the session goes idle.
                    let mut newest_ingested: Option<i64> = None;
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
                                newest_ingested = Some(new_last.timestamp());
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
                    // Personal goals: a play that newly crosses its target
                    // emits one `goal-reached` toast event (the UI listener
                    // only shows a toast; it never rebuilds the overview).
                    if let Some(sid) = state.profile().ok().flatten().map(|p| p.steam_id) {
                        let goals = state.store.scenario_goals(&sid).unwrap_or_default();
                        if !goals.is_empty() {
                            let batch =
                                state.store.latest_plays(&sid, inserted).unwrap_or_default();
                            let crossings =
                                batch_goal_crossings(&batch, &goals, |scenario, before| {
                                    state
                                        .store
                                        .scenario_best_before(&sid, scenario, before)
                                        .unwrap_or(0.0)
                                });
                            for crossing in crossings {
                                let _ = app.emit(
                                    "goal-reached",
                                    serde_json::json!({
                                        "scenario": crossing.scenario,
                                        "score": crossing.score,
                                        "target": crossing.target,
                                    }),
                                );
                            }
                        }
                    }
                    // Arm the quiet post-session sync (re-arms after every
                    // new batch; the next idle gap fires exactly one sync).
                    if let Some(ts) = newest_ingested {
                        last_play_ts = Some(last_play_ts.map_or(ts, |prev| prev.max(ts)));
                        session_active = true;
                    }
                }
            }
            // Quiescence detection: the existing session summary only fires
            // when the NEXT play batch arrives after a >gap pause, so it can
            // never notice the player merely stopping. Instead, once an
            // ingested session has been idle for the gap, run one quiet smart
            // sync (deep off: played benchmarks + snapshots for the new
            // plays). No profile -> skip silently. `session_active` is
            // cleared so this fires once per session; a new play re-arms it.
            if session_closed(
                last_play_ts,
                chrono::Utc::now().timestamp(),
                session_gap_secs(),
                session_active,
            ) {
                session_active = false;
                if let Some(steam_id) = state.profile().ok().flatten().map(|p| p.steam_id) {
                    let app_handle = app.clone();
                    let store = state.store.clone();
                    let registry = state.registry;
                    let stats_dir = state.stats_dir();
                    tauri::async_runtime::spawn(async move {
                        match run_smart_sync(
                            store,
                            registry,
                            steam_id,
                            stats_dir,
                            false,
                            "sync_auto",
                        )
                        .await
                        {
                            Ok(report) => {
                                let _ = app_handle.emit(
                                    "sync-quiet-finished",
                                    serde_json::json!({
                                        "ok": report.ok,
                                        "failed": report.failed,
                                    }),
                                );
                            }
                            Err(e) => eprintln!("auto-sync skipped: {e}"),
                        }
                    });
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

    const GAP: i64 = SESSION_GAP_SECS;

    /// An active session whose newest play is at least a gap old is closed.
    #[test]
    fn session_closed_fires_after_gap_when_session_active() {
        let last = 1_700_000_000;
        assert!(!session_closed(Some(last), last + GAP - 1, GAP, true));
        assert!(session_closed(Some(last), last + GAP, GAP, true));
        assert!(session_closed(Some(last), last + 2 * GAP, GAP, true));
    }

    /// No session was ingested this run: nothing to sync, no matter the age.
    #[test]
    fn session_closed_never_fires_without_active_session() {
        let last = 1_700_000_000;
        assert!(!session_closed(Some(last), last + 10 * GAP, GAP, false));
    }

    /// A play timestamp in the future (clock skew) never counts as closed.
    #[test]
    fn session_closed_ignores_backwards_clock() {
        let last = 1_700_000_000;
        assert!(!session_closed(Some(last), last - GAP, GAP, true));
        assert!(!session_closed(Some(last), last - 1, GAP, true));
    }

    /// No ingested play timestamp at all: never closed.
    #[test]
    fn session_closed_never_fires_without_last_play() {
        assert!(!session_closed(None, 1_700_000_000, GAP, true));
    }

    /// The loop clears `session_active` after firing, so a quiet session is
    /// not re-synced on every subsequent poll tick.
    #[test]
    fn session_closed_stays_quiet_once_cleared() {
        let last = 1_700_000_000;
        let now = last + 2 * GAP;
        assert!(session_closed(Some(last), now, GAP, true));
        // After the loop's one-shot guard clears the flag...
        assert!(!session_closed(Some(last), now, GAP, false));
        // ...a later play re-arms it.
        assert!(session_closed(Some(now), now + GAP, GAP, true));
    }

    fn batch_play(scenario: &str, secs: i64, score: f64) -> kovaaks_core::PlayRecord {
        use chrono::TimeZone;
        kovaaks_core::PlayRecord {
            scenario: scenario.to_string(),
            played_at: chrono::Utc.timestamp_opt(secs, 0).single().expect("ts"),
            score,
            hit_count: 100,
            avg_fps: 240.0,
            source: kovaaks_core::types::PlaySource::Csv,
        }
    }

    fn goal(scenario: &str, target: f64) -> kovaaks_core::store::ScenarioGoal {
        kovaaks_core::store::ScenarioGoal {
            scenario: scenario.to_string(),
            target,
            deadline: None,
        }
    }

    /// A batch that pushes a scenario over its target is a crossing once.
    #[test]
    fn batch_goal_crossings_flags_a_newly_crossed_target() {
        let batch = vec![
            batch_play("VT Pasu", 1_700_000_100, 710.0),
            batch_play("VT Pasu", 1_700_000_200, 725.0),
            batch_play("Gridshot", 1_700_000_300, 1100.0),
        ];
        let goals = vec![goal("VT Pasu", 720.0), goal("Gridshot", 1200.0)];
        let crossings = batch_goal_crossings(&batch, &goals, |scenario, _| {
            if scenario == "VT Pasu" {
                700.0
            } else {
                0.0
            }
        });
        assert_eq!(
            crossings,
            vec![GoalCrossing {
                scenario: "VT Pasu".to_string(),
                score: 725.0,
                target: 720.0,
            }],
            "only Pasu crossed, and only once despite two plays"
        );
    }

    /// A goal that was already reached before this batch must not re-toast.
    #[test]
    fn batch_goal_crossings_ignores_already_reached_goals() {
        let batch = vec![batch_play("VT Pasu", 1_700_000_200, 725.0)];
        let goals = vec![goal("VT Pasu", 720.0)];
        let crossings = batch_goal_crossings(&batch, &goals, |_, _| 730.0);
        assert!(crossings.is_empty());
    }

    /// A score exactly equal to the target crosses (a target means "reach at
    /// least this"), and the batch high is what gets reported.
    #[test]
    fn batch_goal_crossings_accepts_exact_equality() {
        let batch = vec![
            batch_play("VT Pasu", 1_700_000_100, 700.0),
            batch_play("VT Pasu", 1_700_000_200, 720.0),
        ];
        let goals = vec![goal("VT Pasu", 720.0)];
        let crossings = batch_goal_crossings(&batch, &goals, |_, _| 700.0);
        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0].score, 720.0);
    }
}
