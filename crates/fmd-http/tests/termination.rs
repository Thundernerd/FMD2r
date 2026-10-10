// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use common::{StubTransport, TestServer, response};
use fmd_http::{HttpClient, HttpSession};

fn server() -> TestServer {
    TestServer::start(
        Router::new()
            .route("/502", get(|| async { (StatusCode::BAD_GATEWAY, "down") }))
            .route(
                "/hang",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    "late"
                }),
            ),
    )
}

/// Runs `get(url)` on a thread, terminates it after `after`, and returns the result and
/// how long the call took after termination.
fn terminate_during(mut session: HttpSession, url: String, after: Duration) -> (bool, Duration) {
    let token = session.terminate_token();
    let worker = std::thread::spawn(move || {
        let ok = session.get(&url).unwrap();
        (ok, Instant::now())
    });
    std::thread::sleep(after);
    let terminated_at = Instant::now();
    token.terminate();
    let (ok, finished_at) = worker.join().unwrap();
    (ok, finished_at.saturating_duration_since(terminated_at))
}

#[test]
fn termination_stops_an_infinite_retry_loop() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session.set_retry_count(-1);

    let (ok, took) = terminate_during(session, server.url("/502"), Duration::from_millis(200));

    assert!(!ok);
    assert!(took < Duration::from_secs(1), "{took:?}");
}

#[test]
fn termination_aborts_an_in_flight_request() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let session = client.session();

    let (ok, took) = terminate_during(session, server.url("/hang"), Duration::from_millis(200));

    assert!(!ok);
    assert!(took < Duration::from_secs(1), "{took:?}");
}

#[test]
fn termination_aborts_a_connection_queue_wait() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    module.set_max_connections(1);
    let mut holder = client.session_for(&module);
    let holder_token = holder.terminate_token();
    let url = server.url("/hang");
    let holder_thread = std::thread::spawn(move || holder.get(&url).unwrap());
    std::thread::sleep(Duration::from_millis(100));

    let (ok, took) = terminate_during(
        client.session_for(&module),
        server.url("/hang"),
        Duration::from_millis(200),
    );

    assert!(!ok);
    assert!(took < Duration::from_secs(1), "{took:?}");
    holder_token.terminate();
    holder_thread.join().unwrap();
}

#[test]
fn a_terminated_session_sends_nothing() {
    let stub = StubTransport::new(vec![response(200, &[], b"ok")]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.terminate_token().terminate();

    assert!(!session.get("example.test/x").unwrap());
    assert!(session.terminated());
    assert!(stub.requests().is_empty());
}

#[test]
fn sessions_can_share_their_owner_terminate_token() {
    let stub = StubTransport::new(vec![]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let owner = fmd_http::TerminateToken::new();
    let mut session = client.session();
    session.set_terminate_token(owner.clone());

    owner.terminate();

    assert!(session.terminated());
}
