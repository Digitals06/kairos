//! Crate error type (plan Task 1.1).

use thiserror::Error;

/// Errors surfaced by `kovaaks-core`.
#[derive(Debug, Error)]
pub enum Error {
    /// HTTP client failure (network down, TLS, timeout, ...).
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// KovaaK's webapp-backend responded but the body could not be decoded.
    #[error("invalid API response: {0}")]
    ApiShape(String),

    /// A remote endpoint answered 2xx but its JSON body failed to
    /// deserialize into the expected type.
    #[error("json decoding failed: {0}")]
    Json(#[from] serde_json::Error),

    /// A Steam identifier did not resolve to a player.
    #[error("steam identifier not found: {0}")]
    SteamNotFound(String),

    /// A SteamID64 that is not exactly 17 digits.
    #[error("invalid steam id (expected 17 digits): {0}")]
    InvalidSteamId(String),

    /// The request was rate limited (429) or the server errored (5xx) after
    /// retries were exhausted. `retry_after_secs` carries the server's
    /// `Retry-After` hint when one was present, so callers can cool down
    /// instead of hammering.
    #[error("rate limited or server unavailable: {status}")]
    RateLimited {
        status: u16,
        retry_after_secs: Option<u64>,
    },

    /// Benchmark or difficulty not present in the embedded registry.
    #[error("benchmark not found in registry: {0}")]
    BenchmarkNotFound(String),

    /// SQLite storage failure.
    #[error("storage error: {0}")]
    Store(#[from] rusqlite::Error),

    /// Local stats CSV could not be parsed or read.
    #[error("csv ingest error: {0}")]
    Csv(String),

    /// A UTF-8 or JSON decoding failure on embedded/remote data.
    #[error("decoding error: {0}")]
    Decode(String),
}

/// Convenience alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages_are_stable() {
        let e = Error::InvalidSteamId("12345".to_string());
        assert_eq!(
            e.to_string(),
            "invalid steam id (expected 17 digits): 12345"
        );
        let e = Error::BenchmarkNotFound("Nope".to_string());
        assert!(e.to_string().contains("Nope"));
    }

    #[test]
    fn reqwest_error_converts_via_from() {
        // Compile-time assertion: any reqwest error must lift into
        // Error::Http without manual mapping (Task 1.3 relies on this).
        fn _assert(e: reqwest::Error) -> Error {
            e.into()
        }
    }
}
