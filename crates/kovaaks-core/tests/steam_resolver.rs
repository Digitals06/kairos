//! Integration tests for the evxl steam identity resolver (plan Task 1.4).
//!
//! Offline tests run in the normal `cargo test` pass; the live probe is
//! `#[ignore]`d and run with `cargo test -- --ignored`.

use kovaaks_core::{Error, EvxlClient};

/// Serve exactly one raw HTTP response on a loopback port, then stop.
/// Dependency-free stand-in for a mock server (std + tokio only): the client
/// reads request headers + body, we answer with a canned status.
async fn serve_once(listener: tokio::net::TcpListener, response: String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut sock, _) = listener.accept().await.expect("client must connect");
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    // Read until end of headers.
    let header_end = loop {
        let n = sock.read(&mut tmp).await.expect("read headers");
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4) {
            break pos;
        }
        if n == 0 {
            break buf.len();
        }
    };
    // Drain the declared body so the client's write side never resets.
    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_length: usize = headers
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let n = sock.read(&mut tmp).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    sock.write_all(response.as_bytes())
        .await
        .expect("write response");
    sock.flush().await.ok();
}

/// A 404 from the resolver endpoint must surface as an `Error`, not panic or
/// fabricate a profile (offline test: loopback mock server, no new deps).
#[tokio::test]
async fn not_found_response_maps_to_steam_not_found_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(serve_once(
        listener,
        "HTTP/1.1 404 Not Found\r\n\
         Content-Type: application/json\r\n\
         Content-Length: 21\r\n\
         Connection: close\r\n\
         \r\n\
         {\"detail\":\"not found\"}"
            .to_string(),
    ));
    let client = EvxlClient::new()
        .expect("client")
        .with_base_url(format!("http://{addr}"));
    let err = client
        .resolve("76561190000000001")
        .await
        .expect_err("404 must map to an Error");
    assert!(
        matches!(err, Error::SteamNotFound(ref id) if id == "76561190000000001"),
        "expected SteamNotFound, got {err:?}"
    );
    server.await.expect("mock server task");
}

/// A 5xx must map to `RateLimited` (covers "server errored" family).
#[tokio::test]
async fn server_error_response_maps_to_rate_limited() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(serve_once(
        listener,
        "HTTP/1.1 500 Internal Server Error\r\n\
         Content-Length: 0\r\n\
         Connection: close\r\n\
         \r\n"
            .to_string(),
    ));
    let client = EvxlClient::new()
        .expect("client")
        .with_base_url(format!("http://{addr}"));
    let err = client
        .resolve("76561190000000001")
        .await
        .expect_err("500 must map to an Error");
    assert!(
        matches!(err, Error::RateLimited { status: 500 }),
        "got {err:?}"
    );
    server.await.expect("mock server task");
}

/// Live probe against evxl.app (ground truth, plan recon 2026-09-02:
/// 76561190000000001 resolves to persona "Digitals", FR).
#[tokio::test]
#[ignore]
async fn live_resolves_verified_steam_id_to_digitals() {
    let client = EvxlClient::new().expect("client");
    let profile = client
        .resolve("76561190000000001")
        .await
        .expect("live evxl resolve must succeed");
    assert_eq!(profile.persona, "Digitals");
    assert_eq!(profile.steam_id, "76561190000000001");
}
