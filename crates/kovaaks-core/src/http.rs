//! Shared HTTP client construction (plan Task 1.3).
//!
//! Every remote client in this crate goes through one builder so the
//! user-agent and timeout policy are consistent across kovaaks.com and
//! evxl.app traffic.

use std::time::Duration;

/// Identifies the companion app to public endpoints.
pub const USER_AGENT: &str = "kovaaks-companion/0.1";

/// Hard ceiling for a single request; endpoints here are small JSON docs.
const TIMEOUT_SECS: u64 = 15;

/// Build the shared `reqwest::Client` (UA + 15s timeout, rustls TLS from the
/// workspace feature pin).
pub(crate) fn shared_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()
        .expect("reqwest client with static config must build")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_is_app_identifier() {
        assert_eq!(USER_AGENT, "kovaaks-companion/0.1");
    }

    #[test]
    fn shared_client_builds_offline() {
        // Client construction performs no I/O; must succeed on any machine.
        let _ = shared_client();
    }
}
