//! Client for the KovaaK's public webapp-backend (plan Task 1.3).
//!
//! Endpoints here are unauthenticated; identity is the 17-digit SteamID64,
//! validated locally before any request is made.

use crate::error::{Error, Result};
use crate::http::shared_client;
use crate::types::BenchmarkProgress;

/// Base URL of the KovaaK's webapp-backend (ground truth, plan recon
/// 2026-09-02: public, no auth).
const BASE_URL: &str = "https://kovaaks.com/webapp-backend";

/// HTTP client for kovaaks.com webapp-backend endpoints.
#[derive(Debug, Clone)]
pub struct KovaaksClient {
    http: reqwest::Client,
}

impl KovaaksClient {
    /// Construct with the crate-shared reqwest client (UA + 15s timeout).
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: shared_client(),
        })
    }

    /// A SteamID64 must be exactly 17 ASCII digits; anything else is rejected
    /// locally (no network round-trip) with [`Error::InvalidSteamId`].
    pub fn validate_steam_id(steam_id: &str) -> Result<()> {
        let is_17_digits =
            steam_id.chars().count() == 17 && steam_id.chars().all(|c| c.is_ascii_digit());
        if is_17_digits {
            Ok(())
        } else {
            Err(Error::InvalidSteamId(steam_id.to_string()))
        }
    }

    /// Fetch one player's progress on one benchmark (e.g. VT S5 Novice =
    /// webapp-backend id 459).
    pub async fn benchmark_progress(
        &self,
        benchmark_id: i64,
        steam_id: &str,
    ) -> Result<BenchmarkProgress> {
        Self::validate_steam_id(steam_id)?;
        let url = format!(
            "{BASE_URL}/benchmarks/player-progress-rank-benchmark\
             ?benchmarkId={benchmark_id}&steamId={steam_id}&page=0&max=100"
        );
        let response = self.http.get(&url).send().await?.error_for_status()?;
        // Decode via text + explicit parse so JSON shape failures surface as
        // Error::Json, not Error::Http.
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seventeen_digit_ids_validate() {
        assert!(KovaaksClient::validate_steam_id("76561190000000001").is_ok());
    }

    #[test]
    fn short_alpha_and_empty_ids_are_rejected() {
        for bad in ["12345", "abc", ""] {
            let err = KovaaksClient::validate_steam_id(bad)
                .expect_err("non-17-digit id must be rejected");
            assert!(matches!(err, Error::InvalidSteamId(ref got) if got == bad));
        }
    }

    #[test]
    fn seventeen_chars_with_non_digit_is_rejected() {
        let err = KovaaksClient::validate_steam_id("7656119817333526a")
            .expect_err("17 chars but not all digits");
        assert!(matches!(err, Error::InvalidSteamId(_)));
    }
}
