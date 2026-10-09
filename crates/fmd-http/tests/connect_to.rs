// Helpers outside #[test] fns are not covered by clippy.toml's test exemption; unwrap is fine in tests/.
#![allow(clippy::unwrap_used)]

mod common;

use std::net::SocketAddr;

use axum::Router;
use axum::routing::any;
use common::{TestServer, echo, echoed_header};
use fmd_http::HttpClient;

#[test]
fn a_pinned_address_is_connected_to_without_resolving_the_host() {
    // `pinned.invalid` cannot resolve: the request only arrives because it goes to the pinned
    // address, and it still names the URL's host.
    let server = TestServer::start(Router::new().route("/echo", any(echo)));
    let addr: SocketAddr = server.base.trim_start_matches("http://").parse().unwrap();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.set_connect_to(Some(addr));

    let url = format!("http://pinned.invalid:{}/echo", addr.port());
    assert!(session.get(&url).unwrap());

    assert_eq!(session.result_code(), 200);
    assert_eq!(
        echoed_header(session.document(), "host"),
        Some(format!("pinned.invalid:{}", addr.port()))
    );
}
