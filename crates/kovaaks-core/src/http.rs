//! Shared HTTP client construction (plan Task 1.3).
//!
//! Every remote client in this crate goes through one builder so the
//! user-agent and timeout policy are consistent across kovaaks.com and
//! evxl.app traffic.

use std::time::Duration;

/// Identifies the app to public endpoints (name tracks the workspace version).
pub const USER_AGENT: &str = concat!("kairos/", env!("CARGO_PKG_VERSION"));

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
        assert!(USER_AGENT.starts_with("kairos/"), "{USER_AGENT}");
    }

    #[test]
    fn shared_client_builds_offline() {
        // Client construction performs no I/O; must succeed on any machine.
        let _ = shared_client();
    }
}
