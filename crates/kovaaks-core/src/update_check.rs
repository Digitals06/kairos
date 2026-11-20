//! In-app update check (v0.6.0): compare the running version with the latest
//! GitHub release tag.
//!
//! Read-only by design: nothing is downloaded or installed; the app only
//! surfaces a pointer to the release page. The network call lives in this
//! crate so it reuses the shared HTTP client (same UA/timeout/TLS stack as the
//! KovaaK's sync client); the comparison and cache-expiry helpers are pure for
//! unit testing.

use chrono::{DateTime, Utc};

/// Human-facing release page (used when no specific tag is cached).
pub const RELEASES_PAGE: &str = "https://github.com/Digitals06/kairos/releases/latest";

/// Public GitHub API endpoint for the newest published release.
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/Digitals06/kairos/releases/latest";

/// How long a stored check result stays fresh before launch-time checking
/// asks GitHub again.
pub const CACHE_TTL_HOURS: i64 = 24;

/// Minimal slice of the GitHub release payload the app needs.
#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
}

/// True when `latest` is strictly newer than `current`.
///
/// Versions are dotted numeric (`0.6.0`); a tag's leading `v` is removed by
/// [`tag_version`] before this runs. Equal, older, or malformed input is
/// never "newer", so a bad GitHub payload can't flip the chip on.
pub fn newer_than(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => normalize(latest) > normalize(current),
        _ => false,
    }
}

/// Strip the conventional `v` prefix from a release tag and validate the rest
/// as a dotted-numeric version. Malformed tags yield `None` (silent skip).
pub fn tag_version(tag: &str) -> Option<String> {
    let trimmed = tag.trim();
    let numeric = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    parse_version(numeric)?;
    Some(numeric.to_string())
}

/// Web page for one specific release, e.g.
/// `https://github.com/Digitals06/kairos/releases/tag/v0.6.0`.
///
/// `version` is a dotted-numeric version without the tag's `v` prefix (the
/// shape [`tag_version`] returns), so callers never rebuild the repo URL.
pub fn release_tag_url(version: &str) -> String {
    format!("https://github.com/Digitals06/kairos/releases/tag/v{version}")
}

/// True while a stored RFC3339 check timestamp is younger than the cache TTL.
/// A missing/malformed timestamp is stale (safe default: check again).
pub fn cache_is_fresh(checked_at: &str, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(checked_at)
        .map(|then| {
            now.signed_duration_since(then.with_timezone(&Utc))
                < chrono::Duration::hours(CACHE_TTL_HOURS)
        })
        .unwrap_or(false)
}

/// Ask GitHub for the newest release tag. Returns `Ok(None)` when the payload
/// is not a version (silently treat as "no update"), and `Err` on network
/// trouble so the caller can log-and-ignore.
pub async fn fetch_latest_version() -> Result<Option<String>, crate::Error> {
    let client = crate::http::shared_client();
    let body = client
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_latest_release(&body))
}

/// Parse a GitHub `releases/latest` body into a comparable version string.
pub fn parse_latest_release(json: &str) -> Option<String> {
    let release: Release = serde_json::from_str(json).ok()?;
    tag_version(&release.tag_name)
}

fn parse_version(raw: &str) -> Option<Vec<u64>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

/// Drop trailing zero components so `0.5` and `0.5.0` compare equal.
fn normalize(mut parts: Vec<u64>) -> Vec<u64> {
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_than_accepts_a_newer_release() {
        assert!(newer_than("0.6.0", "0.5.0"));
        assert!(newer_than("0.6.0", "0.5.9"));
        assert!(newer_than("1.0.0", "0.99.99"));
    }

    #[test]
    fn newer_than_treats_equal_and_older_as_current() {
        assert!(!newer_than("0.5.0", "0.5.0"));
        assert!(!newer_than("0.4.9", "0.5.0"));
        assert!(!newer_than("0.5", "0.5.0"));
    }

    #[test]
    fn newer_than_compares_segments_numerically() {
        // String comparison would put "0.10.0" before "0.9.0".
        assert!(newer_than("0.10.0", "0.9.0"));
        assert!(newer_than("0.9.10", "0.9.9"));
        assert!(!newer_than("0.9.0", "0.10.0"));
    }

    #[test]
    fn newer_than_rejects_malformed_versions() {
        for bad in ["", "v0.6.0", "0.6.0-beta", "0..1", "0.6.x", "-1.0.0"] {
            assert!(!newer_than(bad, "0.5.0"), "latest {bad:?}");
            assert!(!newer_than("0.6.0", bad), "current {bad:?}");
        }
    }

    #[test]
    fn tag_version_strips_conventional_v_prefix() {
        assert_eq!(tag_version("v0.6.0").as_deref(), Some("0.6.0"));
        assert_eq!(tag_version("V1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(tag_version("0.6.0").as_deref(), Some("0.6.0"));
        assert_eq!(tag_version("release-0.6.0"), None);
    }

    #[test]
    fn parse_release_reads_the_tag_name() {
        let json = r#"{"tag_name":"v0.6.0","name":"v0.6.0"}"#;
        assert_eq!(parse_latest_release(json).as_deref(), Some("0.6.0"));
        assert_eq!(parse_latest_release("{}"), None);
        assert_eq!(parse_latest_release("not json"), None);
        assert_eq!(parse_latest_release(r#"{"tag_name":"nightly"}"#), None);
    }

    #[test]
    fn release_tag_url_points_at_the_version_tag() {
        assert_eq!(
            release_tag_url("0.6.0"),
            "https://github.com/Digitals06/kairos/releases/tag/v0.6.0"
        );
    }

    #[test]
    fn cache_is_fresh_within_the_ttl() {
        let now = DateTime::parse_from_rfc3339("2026-11-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(cache_is_fresh("2026-11-20T11:00:00Z", now));
        assert!(cache_is_fresh("2026-11-19T12:01:00Z", now));
    }

    #[test]
    fn cache_is_stale_after_the_ttl_or_when_malformed() {
        let now = DateTime::parse_from_rfc3339("2026-11-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!cache_is_fresh("2026-11-19T11:59:00Z", now));
        assert!(!cache_is_fresh("yesterday", now));
        assert!(!cache_is_fresh("", now));
    }
}
