//! Unit tests for the scenario-history builder with local-play fallback:
//! scenarios without a synced (score > 0) entry must still chart from local
//! CSV plays.
use kovaaks_core::metrics::build_scenario_history;

use chrono::TimeZone;

fn utc(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.timestamp_opt(secs, 0).unwrap()
}

/// Minimal StoredScenario/StoredSnapshot stand-ins via the real types.
fn snap(
    captured_at: chrono::DateTime<chrono::Utc>,
    rows: &[(&str, &str, i64)],
) -> kovaaks_core::store::StoredSnapshot {
    use kovaaks_core::store::{StoredScenario, StoredSnapshot};
    StoredSnapshot {
        id: 0,
        steam_id: String::new(),
        benchmark_id: 0,
        captured_at,
        benchmark_progress: 0,
        overall_rank: 0,
        scenarios: rows
            .iter()
            .map(|(scenario, category, score)| StoredScenario {
                scenario: (*scenario).to_string(),
                category: (*category).to_string(),
                score: *score,
                leaderboard_rank: 0,
                scenario_rank: 0,
                category_rank: 0,
                api_order: 0,
                rank_maxes: Vec::new(),
            })
            .collect(),
    }
}

fn plays(rows: &[(&str, i64, f64)]) -> Vec<(String, chrono::DateTime<chrono::Utc>, f64)> {
    rows.iter()
        .map(|(s, t, score)| ((*s).to_string(), utc(*t), *score))
        .collect()
}

#[test]
fn snapshot_series_still_win_over_local_plays() {
    let history = vec![
        snap(utc(100), &[("GridShot", "Clicking", 1000)]),
        snap(utc(200), &[("GridShot", "Clicking", 1200)]),
    ];
    let local = plays(&[("GridShot", 150, 99999.0)]);
    let out = build_scenario_history(&history, &local);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source.as_str(), "snapshot");
    // The 99999 local play must NOT leak into a snapshot-backed series.
    assert_eq!(out[0].points.len(), 2);
    assert_eq!(out[0].points[1].1, 1200);
}

#[test]
fn zero_score_scenario_falls_back_to_local_plays() {
    // Synced snapshot has the scenario with score 0 (never played around sync),
    // local CSVs contain real plays.
    let history = vec![snap(utc(300), &[("JennClick", "Evasive", 0)])];
    let local = plays(&[
        ("JennClick", 100, 800.0),
        ("JennClick", 200, 805.6),
        ("JennClick", 250, 700.0), // not an improvement — filtered
    ]);
    let out = build_scenario_history(&history, &local);
    assert_eq!(out.len(), 1, "scenario must survive with local points");
    assert_eq!(out[0].source.as_str(), "local");
    assert_eq!(out[0].points.len(), 2, "only new highs kept");
    assert_eq!(out[0].points[0].1, 800);
    // 805.6 rounds (not truncates) to 806 — matching how the sync echoes it.
    assert_eq!(out[0].points[1].1, 806);
    assert_eq!(out[0].category, "Evasive", "category from the snapshot row");
}

#[test]
fn scenario_never_synced_but_played_locally_appears() {
    // Benchmark snapshot exists but lacks the scenario entirely (API omitted);
    // local plays prove it was played.
    let history = vec![snap(utc(100), &[("GridShot", "Clicking", 900)])];
    let local = plays(&[("Spidershot", 50, 500.0), ("Spidershot", 150, 650.0)]);
    let out = build_scenario_history(&history, &local);
    let spiders = out.iter().find(|s| s.scenario == "Spidershot");
    assert!(spiders.is_some(), "local-only scenario must appear");
    let s = spiders.unwrap();
    assert_eq!(s.source.as_str(), "local");
    assert_eq!(s.points.len(), 2);
    assert_eq!(
        s.category, "Local",
        "no snapshot row → placeholder category"
    );
}

#[test]
fn unplayed_scenario_without_plays_is_dropped() {
    let history = vec![snap(utc(100), &[("GridShot", "Clicking", 900)])];
    let out = build_scenario_history(&history, &[]);
    assert_eq!(out.len(), 1, "zero-score scenario without plays: no chart");
}

#[test]
fn order_follows_latest_snapshot_then_locals_appended() {
    // Bounceshot is synced but unplayed (score 0, no local plays) → dropped.
    let history = vec![snap(
        utc(100),
        &[("GridShot", "Clicking", 900), ("Bounceshot", "Clicking", 0)],
    )];
    let local = plays(&[("Spidershot", 50, 500.0)]);
    let out = build_scenario_history(&history, &local);
    let names: Vec<&str> = out.iter().map(|s| s.scenario.as_str()).collect();
    assert_eq!(names, vec!["GridShot", "Spidershot"]);
}

#[test]
fn snapshot_series_drops_points_echoing_a_play_score() {
    // Sync captured 1180 at t=200 (echo of the 1180 play at t=100? No —
    // play 1180 is at t=150, BEFORE the sync): the snapshot point must be
    // dropped and the series stays snapshot-backed via the other point.
    let history = vec![
        snap(utc(200), &[("S", "Cat", 1180)]),
        snap(utc(300), &[("S", "Cat", 1200)]),
    ];
    let local = plays(&[("S", 150, 1180.0)]);
    let out = build_scenario_history(&history, &local);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source.as_str(), "snapshot");
    let scores: Vec<i64> = out[0].points.iter().map(|p| p.1).collect();
    assert_eq!(scores, vec![1200], "echoed 1180 snapshot point dropped");
}

#[test]
fn snapshot_series_falls_back_to_local_when_all_points_echo_plays() {
    // Only snapshot point echoes the play → series becomes local-backed.
    let history = vec![snap(utc(200), &[("S", "Cat", 1180)])];
    let local = plays(&[("S", 150, 1180.0)]);
    let out = build_scenario_history(&history, &local);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source.as_str(), "local");
    assert_eq!(out[0].points.len(), 1);
    assert_eq!(out[0].points[0].1, 1180);
}
