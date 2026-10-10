// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::{any, get};
use common::{StubTransport, TestServer, echo, echoed_header, response};
use fmd_http::{HttpClient, Proxy, ProxyKind};

fn proxy(kind: ProxyKind, host: &str, port: &str) -> Proxy {
    Proxy {
        kind,
        host: host.into(),
        port: port.into(),
        user: String::new(),
        pass: String::new(),
    }
}

#[test]
fn http_proxy_receives_the_request() {
    // The test server plays the proxy and routes the absolute-form request by path.
    let server = TestServer::start(Router::new().route("/echo", any(echo)));
    let port = server.base.rsplit(':').next().unwrap().to_string();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.set_proxy("http", "127.0.0.1", &port, "", "");

    assert!(session.get("http://manga.invalid/echo").unwrap());

    assert_eq!(
        echoed_header(session.document(), "host").as_deref(),
        Some("manga.invalid")
    );
}

#[test]
fn set_proxy_accepts_http_and_socks_types_case_insensitively() {
    let client = HttpClient::with_transport(StubTransport::new(vec![])).unwrap();
    let mut session = client.session();

    session.set_proxy("socks4", "10.0.0.1", "1080", "u", "p");
    assert_eq!(
        session.proxy(),
        Some(Proxy {
            kind: ProxyKind::Socks4,
            host: "10.0.0.1".into(),
            port: "1080".into(),
            user: "u".into(),
            pass: "p".into(),
        })
    );
    session.set_proxy("SOCKS5", "10.0.0.2", "9050", "", "");
    assert_eq!(
        session.proxy(),
        Some(proxy(ProxyKind::Socks5, "10.0.0.2", "9050"))
    );
    session.set_proxy("ftp", "10.0.0.3", "21", "", "");
    assert_eq!(session.proxy(), None);
}

#[test]
fn the_session_proxy_is_passed_to_the_transport() {
    let stub = StubTransport::new(vec![response(200, &[], b"ok")]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_proxy("SOCKS5", "10.0.0.2", "9050", "", "");

    session.get("example.test/").unwrap();

    assert_eq!(
        stub.requests()[0].proxy,
        Some(proxy(ProxyKind::Socks5, "10.0.0.2", "9050"))
    );
}

#[test]
fn changing_a_default_applies_to_existing_and_new_sessions() {
    let client = HttpClient::with_transport(StubTransport::new(vec![])).unwrap();
    let mut existing = client.session();
    existing.set_proxy("HTTP", "own", "1", "", "");
    existing.set_retry_count(7);

    client.set_default_proxy(Some(proxy(ProxyKind::Http, "global", "8080")));
    client.set_default_retry_count(2);
    client.set_default_timeout(1000);
    let new = client.session();

    for s in [&existing, &new] {
        assert_eq!(s.proxy(), Some(proxy(ProxyKind::Http, "global", "8080")));
        assert_eq!(s.retry_count(), 2);
        assert_eq!(s.timeout(), 1000);
    }
}

#[test]
fn a_session_override_made_after_the_default_wins() {
    let client = HttpClient::with_transport(StubTransport::new(vec![])).unwrap();
    client.set_default_retry_count(2);
    let mut session = client.session();

    session.set_retry_count(5);
    // Setting a default to its current value applies nothing.
    client.set_default_retry_count(2);

    assert_eq!(session.retry_count(), 5);
}

#[test]
fn fmd2_defaults_for_a_new_session() {
    let client = HttpClient::with_transport(StubTransport::new(vec![])).unwrap();
    let session = client.session();

    assert_eq!(session.retry_count(), 0);
    assert_eq!(session.timeout(), 15000);
    assert_eq!(session.proxy(), None);
    assert_eq!(session.mime_type(), "text/html");
}

#[test]
fn a_blank_default_user_agent_falls_back_to_synapse() {
    let server = TestServer::start(Router::new().route("/echo", any(echo)));
    let client = HttpClient::new().unwrap();
    client.set_default_user_agent("  ");
    let mut session = client.session();

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "user-agent").as_deref(),
        Some("Mozilla/4.0 (compatible; Synapse)")
    );
}

#[test]
fn the_user_agent_can_be_changed_per_session() {
    let server = TestServer::start(Router::new().route("/echo", any(echo)));
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.set_user_agent("curl/7.70.0");

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "user-agent").as_deref(),
        Some("curl/7.70.0")
    );
}

#[test]
fn the_timeout_bounds_a_stalled_response() {
    let server = TestServer::start(Router::new().route(
        "/hang",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            "late"
        }),
    ));
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.set_timeout(200);

    let started = Instant::now();
    assert!(!session.get(&server.url("/hang")).unwrap());

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(session.result_code(), 500);
}
