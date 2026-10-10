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
fn post_turns_a_text_html_content_type_into_a_form_post() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.headers_mut().set_value("Content-Type", "text/html");

    assert!(session.post(&server.url("/echo"), b"a=1").unwrap());

    let doc = session.document();
    assert!(doc.starts_with(b"POST\n"));
    assert_eq!(
        echoed_header(doc, "content-type").as_deref(),
        Some("application/x-www-form-urlencoded; charset=UTF-8")
    );
    assert!(doc.ends_with(b"\n\na=1"));
}

#[test]
fn post_moves_the_content_type_header_into_mime_type() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session
        .headers_mut()
        .set_value("Content-Type", " application/json ");

    session.post(&server.url("/echo"), b"{}").unwrap();

    let text = String::from_utf8_lossy(session.document()).to_string();
    assert_eq!(text.matches("content-type:").count(), 1, "{text}");
    assert_eq!(
        echoed_header(session.document(), "content-type").as_deref(),
        Some("application/json")
    );
}

#[test]
fn post_without_content_type_sends_the_default_mime_type_rewritten() {
    // A fresh session's MimeType is text/html, which POST rewrites.
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    session.post(&server.url("/echo"), b"a=1").unwrap();

    assert_eq!(
        echoed_header(session.document(), "content-type").as_deref(),
        Some("application/x-www-form-urlencoded; charset=UTF-8")
    );
}

#[test]
fn xhr_adds_x_requested_with_and_resets_stale_headers() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.get(&server.url("/echo")).unwrap();

    assert!(session.xhr(&server.url("/echo")).unwrap());

    let doc = session.document();
    assert!(doc.starts_with(b"GET\n"));
    assert_eq!(
        echoed_header(doc, "x-requested-with").as_deref(),
        Some("XMLHttpRequest")
    );
    assert_eq!(echoed_header(doc, "x-echo"), None);
    assert_eq!(echoed_header(doc, "dnt").as_deref(), Some("1"));
}

#[test]
fn head_returns_false_because_there_is_no_body() {
    let server = echo_server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    assert!(!session.head(&server.url("/echo")).unwrap());
    assert_eq!(session.result_code(), 200);
}
