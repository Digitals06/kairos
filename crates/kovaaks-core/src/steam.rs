//! Client for the evxl `/api/steam` identity resolver (plan Task 1.4).
//!
//! Accepts a 17-digit SteamID64, a vanity name, or a profile URL and resolves
//! it to a [`PlayerProfile`].

use crate::error::{Error, Result};
use crate::http::shared_client;
use crate::types::PlayerProfile;

/// Base URL of the evxl app (ground truth, plan recon 2026-09-02: POST
/// `/api/steam` with `{"identifier": s}` is public, no auth).
const BASE_URL: &str = "https://evxl.app";

/// HTTP client for evxl endpoints.
#[derive(Debug, Clone)]
pub struct EvxlClient {
    http: reqwest::Client,
    base_url: String,
}

impl EvxlClient {
    /// Construct with the crate-shared reqwest client (UA + 15s timeout).
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: shared_client(),
            base_url: BASE_URL.to_string(),
        })
    }

    /// Override the base URL (tests point this at a loopback mock server).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Resolve a SteamID64 / vanity name / profile URL to a player profile.
    pub async fn resolve(&self, identifier: &str) -> Result<PlayerProfile> {
        let url = format!("{}/api/steam", self.base_url);
        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "identifier": identifier }))
            .send()
            .await?;
        // Non-2xx: known shapes map to their dedicated error, anything else
        // to RateLimited (which also documents the 429/5xx family).
        let status = response.status();
        if !status.is_success() {
            return match status.as_u16() {
                404 => Err(Error::SteamNotFound(identifier.to_string())),
                s => Err(Error::RateLimited {
                    status: s,
                    retry_after_secs: None,
                }),
            };
        }
        // Decode via text + explicit parse so shape failures surface as
        // Error::Json, not Error::Http.
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_url_is_evxl() {
        let client = EvxlClient::new().expect("client");
        assert_eq!(client.base_url, "https://evxl.app");
    }

    #[test]
    fn with_base_url_overrides_endpoint_prefix() {
        let client = EvxlClient::new()
            .expect("client")
            .with_base_url("http://127.0.0.1:1");
        assert_eq!(client.base_url, "http://127.0.0.1:1");
    }
}
