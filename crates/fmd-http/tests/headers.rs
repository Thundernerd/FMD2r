// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use axum::Router;
use axum::routing::any;
use common::{TestServer, echo, echoed_header};
use fmd_http::HttpClient;

fn echo_server() -> TestServer {
    TestServer::start(Router::new().route("/echo", any(echo)))
}

#[test]
fn response_headers_replace_request_headers_after_the_call() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.headers_mut().set_value("X-Test", "1");

    assert!(session.get(&server.url("/echo")).unwrap());

    assert_eq!(
        echoed_header(session.document(), "x-test").as_deref(),
        Some("1")
    );
    assert!(session.headers().lines()[0].starts_with("HTTP/1.1 200"));
    assert_eq!(session.headers().value("X-Echo").trim(), "yes");
    assert_eq!(session.headers().value("X-Test"), "");
}

#[test]
fn stale_response_headers_are_reset_before_the_next_request() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.get(&server.url("/echo")).unwrap();
    // Appending to the response headers does not stop the reset.
    session.headers_mut().set_value("X-Late", "1");

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(echoed_header(session.document(), "x-echo"), None);
    assert_eq!(echoed_header(session.document(), "x-late"), None);
    assert_eq!(
        echoed_header(session.document(), "dnt").as_deref(),
        Some("1")
    );
}

#[test]
fn a_new_session_sends_fmd2_default_headers() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    session.get(&server.url("/echo")).unwrap();

    let doc = session.document();
    assert_eq!(
        echoed_header(doc, "user-agent").as_deref(),
        Some(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
        )
    );
    assert_eq!(
        echoed_header(doc, "accept-encoding").as_deref(),
        Some("gzip, deflate, br, zstd")
    );
    assert_eq!(
        echoed_header(doc, "accept").as_deref(),
        Some("text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8")
    );
    assert_eq!(
        echoed_header(doc, "accept-language").as_deref(),
        Some("en-US,en;q=0.5")
    );
    assert_eq!(
        echoed_header(doc, "accept-charset").as_deref(),
        Some("utf-8")
    );
    assert_eq!(
        echoed_header(doc, "upgrade-insecure-requests").as_deref(),
        Some("1")
    );
}

#[test]
fn reset_basic_keeps_only_accept_encoding() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.reset_basic();

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(echoed_header(session.document(), "dnt"), None);
    assert_eq!(
        echoed_header(session.document(), "accept-encoding").as_deref(),
        Some("gzip, deflate, br, zstd")
    );
}

#[test]
fn header_names_and_values_are_trimmed() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.headers_mut().push("  X-Spaced :   v  ");

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "x-spaced").as_deref(),
        Some("v")
    );
}
