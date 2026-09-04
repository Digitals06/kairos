//! Dedupe tests: snapshot points that duplicate a local play (same run) must
//! not inflate the combined series — they would drag avg/high improvement
//! toward the sync timestamp instead of the real play time.

use chrono::TimeZone;
use kovaaks_core::metrics::merge_plays_snapshots_dedup;

fn utc(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.timestamp_opt(secs, 0).unwrap()
}

#[test]
fn snapshot_duplicating_local_play_is_dropped() {
    // Player scores 805.6 locally at t=100; sync at t=120 records the same
    // score (same run, 20s later). The snapshot point must be dropped.
    let plays = vec![("S".to_string(), utc(100), 805.6)];
    let snapshots = vec![(utc(120), 805.6), (utc(300), 900.0)];
    let merged = merge_plays_snapshots_dedup(&plays, &snapshots);
    let times: Vec<i64> = merged.iter().map(|(t, _)| t.timestamp()).collect();
    assert_eq!(times, vec![100, 300], "duplicate snapshot point removed");
}

#[test]
fn snapshot_with_new_score_is_kept() {
    let plays = vec![("S".to_string(), utc(100), 805.6)];
    let snapshots = vec![(utc(120), 1100.0)];
    let merged = merge_plays_snapshots_dedup(&plays, &snapshots);
    assert_eq!(merged.len(), 2, "new score = real new run");
}

#[test]
fn same_score_outside_time_window_is_kept() {
    // Same score 2 hours later is a genuine replay, not the same run.
    let plays = vec![("S".to_string(), utc(100), 805.6)];
    let snapshots = vec![(utc(100 + 2 * 3600), 805.6)];
    let merged = merge_plays_snapshots_dedup(&plays, &snapshots);
    assert_eq!(merged.len(), 2);
}

#[test]
fn dedupe_applies_to_improving_snapshots_only_after_filter() {
    // Caller filters snapshots through improving_only first; merge keeps plays
    // verbatim. Two plays, one duplicate snapshot → 2 points.
    let plays = vec![("S".to_string(), utc(100), 800.0)];
    let snapshots = vec![(utc(110), 800.0), (utc(200), 900.0), (utc(210), 900.0)];
    let filtered = kovaaks_core::improving_only(&snapshots);
    let merged = merge_plays_snapshots_dedup(&plays, &filtered);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[1].1, 900.0);
}
