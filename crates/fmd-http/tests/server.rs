// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use common::TestServer;
use fmd_http::HttpClient;

#[test]
fn not_found_with_a_body_counts_as_success() {
    let server = TestServer::start(Router::new().route(
        "/missing",
        get(|| async { (StatusCode::NOT_FOUND, "nope") }),
    ));
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    assert!(session.get(&server.url("/missing")).unwrap());
    assert_eq!(session.result_code(), 404);
    assert_eq!(session.document(), b"nope");
}

#[test]
fn ok_with_an_empty_body_counts_as_failure() {
    let server = TestServer::start(Router::new().route("/empty", get(|| async { "" })));
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    assert!(!session.get(&server.url("/empty")).unwrap());
    assert_eq!(session.result_code(), 200);
}
