//! Integration tests for the KovaaK's webapp-backend client (plan Task 1.3).
//!
//! Offline tests run in the normal `cargo test` pass; live probes are
//! `#[ignore]`d and run with `cargo test -- --ignored`.

use kovaaks_core::KovaaksClient;

/// Steam ids that are not exactly 17 ASCII digits must be rejected locally,
/// before any network activity happens (offline test).
#[tokio::test]
async fn rejects_steam_ids_that_are_not_exactly_17_digits() {
    let client = KovaaksClient::new().expect("client construction needs no network");
    for bad in ["12345", "abc", ""] {
        let err = client
            .benchmark_progress(459, bad)
            .await
            .expect_err("steam id must be rejected before any HTTP call");
        assert!(
            matches!(err, kovaaks_core::Error::InvalidSteamId(ref id) if id == bad),
            "expected InvalidSteamId for {bad:?}"
        );
    }
}

/// Live probe against kovaaks.com (ground truth, plan recon 2026-09-02:
/// VT S5 Novice = webapp-backend benchmark 459; player 76561190000000001
/// had benchmark_progress 180000 / overall_rank 4 at recon time).
#[tokio::test]
#[ignore]
async fn live_fetches_benchmark_progress_for_verified_player() {
    let client = KovaaksClient::new().expect("client");
    let progress = client
        .benchmark_progress(459, "76561190000000001")
        .await
        .expect("live webapp-backend call must succeed");
    assert!(
        progress.overall_rank >= 1,
        "overall_rank is a 1-based tier index, got {}",
        progress.overall_rank
    );
    assert!(
        !progress.categories.is_empty(),
        "expected at least one category (Clicking/Tracking/...)"
    );
}
