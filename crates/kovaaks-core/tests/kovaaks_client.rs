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

/// Live probe against kovaaks.com. Requires KAIROS_TEST_STEAM_ID (a SteamID64
/// with KovaaK's data); skips when unset so CI/other machines never hit it.
#[tokio::test]
#[ignore]
async fn live_fetches_benchmark_progress_for_verified_player() {
    let Ok(sid) = std::env::var("KAIROS_TEST_STEAM_ID") else {
        eprintln!("skipped: KAIROS_TEST_STEAM_ID not set");
        return;
    };
    let client = KovaaksClient::new().expect("client");
    let progress = client
        .benchmark_progress(459, &sid)
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
