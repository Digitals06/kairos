//! Client for the KovaaK's public webapp-backend (plan Task 1.3).
//!
//! Endpoints here are unauthenticated; identity is the 17-digit SteamID64,
//! validated locally before any request is made.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::http::shared_client;
use crate::types::BenchmarkProgress;

/// Base URL of the KovaaK's webapp-backend (ground truth, plan recon
/// 2026-09-02: public, no auth).
const BASE_URL: &str = "https://kovaaks.com/webapp-backend";

/// Minimum spacing between consecutive request starts to kovaaks.com
/// (~4 req/s, well under the observed rate-limit threshold; the sync engine
/// probes with 4-way concurrency, so pacing must live in the client).
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(250);

/// HTTP client for kovaaks.com webapp-backend endpoints. Clones share the
/// same pacing state and connection pool.
#[derive(Debug, Clone)]
pub struct KovaaksClient {
    http: reqwest::Client,
    /// Earliest instant the next request may start (client-wide pacing).
    next_send: std::sync::Arc<Mutex<Instant>>,
}

impl KovaaksClient {
    /// Construct with the crate-shared reqwest client (UA + 15s timeout).
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: shared_client(),
            next_send: std::sync::Arc::new(Mutex::new(Instant::now())),
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

    /// Space request starts at least [`MIN_REQUEST_INTERVAL`] apart across all
    /// concurrent callers (tokio-friendly: no await while holding the lock).
    async fn pace(&self) {
        let wait = {
            let mut next = self.next_send.lock().expect("pace lock not poisoned");
            let now = Instant::now();
            let earliest = (*next).max(now);
            *next = earliest + MIN_REQUEST_INTERVAL;
            earliest.duration_since(now)
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
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
        self.pace().await;
        let url = format!(
            "{BASE_URL}/benchmarks/player-progress-rank-benchmark\
             ?benchmarkId={benchmark_id}&steamId={steam_id}&page=0&max=100"
        );
        let response = self.http.get(&url).send().await?;
        let status = response.status();
        if status.as_u16() == 429 || status.is_server_error() {
            // Surface as RateLimited so the sync engine's retry policy
            // (up to 2 retries on 429/5xx) can engage.
            return Err(Error::RateLimited {
                status: status.as_u16(),
            });
        }
        let response = response.error_for_status()?;
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
