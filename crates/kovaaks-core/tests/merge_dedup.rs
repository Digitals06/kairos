//! Dedupe tests: snapshot points that duplicate a local play (same score = the
//! same run echoed by the sync) must not inflate the combined series.

use chrono::TimeZone;
use kovaaks_core::metrics::merge_plays_snapshots_dedup;

fn utc(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.timestamp_opt(secs, 0).unwrap()
}

#[test]
fn snapshot_duplicating_local_play_is_dropped() {
    // Player scores 805.6 locally at t=100; a sync at t=120 records the same
    // score (same run echoed 20s later). The snapshot point must be dropped.
    let plays = vec![("S".to_string(), utc(100), 805.6)];
    let snapshots = vec![(utc(120), 805.6), (utc(300), 900.0)];
    let merged = merge_plays_snapshots_dedup(&plays, &snapshots);
    let times: Vec<i64> = merged.iter().map(|(t, _)| t.timestamp()).collect();
    assert_eq!(times, vec![100, 300], "duplicate snapshot point removed");
}

#[test]
fn snapshot_duplicating_much_later_is_still_dropped() {
    // The user's case: play at 10:33, sync echoes the same score hours later.
    // Same score to 2dp = same run, regardless of elapsed time.
    let plays = vec![("S".to_string(), utc(100), 1130.0)];
    let snapshots = vec![(utc(100 + 14 * 3600), 1130.0)];
    let merged = merge_plays_snapshots_dedup(&plays, &snapshots);
    assert_eq!(merged.len(), 1, "play kept, snapshot echo dropped");
    assert_eq!(merged[0].1, 1130.0);
}

#[test]
fn snapshot_with_new_score_is_kept() {
    let plays = vec![("S".to_string(), utc(100), 805.6)];
    let snapshots = vec![(utc(120), 1100.0)];
    let merged = merge_plays_snapshots_dedup(&plays, &snapshots);
    assert_eq!(merged.len(), 2, "new score = real new run");
}

#[test]
fn decimal_play_matches_rounded_sync_echo() {
    // The sync echoes plays as rounded integers: play 1558.668 -> echo 1559.
    // The echo must be dropped even though the raw values differ by 0.332.
    let plays = vec![("S".to_string(), utc(100), 1558.668)];
    let snapshots = vec![(utc(120), 1559.0)];
    let merged = merge_plays_snapshots_dedup(&plays, &snapshots);
    assert_eq!(merged.len(), 1, "rounded echo dropped");
    assert_eq!(merged[0].1, 1558.668);
}

#[test]
fn genuinely_different_run_rounds_apart() {
    // 1125.29 vs 1125.71 round to different integers -> two distinct runs.
    let plays = vec![("S".to_string(), utc(100), 1125.29)];
    let snapshots = vec![(utc(120), 1125.71)];
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
