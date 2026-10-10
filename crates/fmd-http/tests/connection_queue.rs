// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use common::TestServer;
use fmd_http::HttpClient;

#[derive(Default)]
struct Concurrency {
    current: AtomicUsize,
    max: AtomicUsize,
}

async fn slow(State(c): State<Arc<Concurrency>>) -> &'static str {
    let now = c.current.fetch_add(1, Ordering::SeqCst) + 1;
    c.max.fetch_max(now, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;
    c.current.fetch_sub(1, Ordering::SeqCst);
    "done"
}

/// Runs one GET per session on its own thread and returns the server's peak concurrency.
fn peak_concurrency(sessions: Vec<fmd_http::HttpSession>) -> usize {
    let concurrency = Arc::new(Concurrency::default());
    let server = TestServer::start(
        Router::new()
            .route("/slow", get(slow))
            .with_state(concurrency.clone()),
    );
    let url = server.url("/slow");
    let threads: Vec<_> = sessions
        .into_iter()
        .map(|mut s| {
            let url = url.clone();
            std::thread::spawn(move || assert!(s.get(&url).unwrap()))
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    concurrency.max.load(Ordering::SeqCst)
}

#[test]
fn a_module_connection_limit_queues_extra_requests() {
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    module.set_max_connections(2);

    let sessions = (0..4).map(|_| client.session_for(&module)).collect();

    assert_eq!(peak_concurrency(sessions), 2);
}

#[test]
fn a_connection_limit_of_zero_is_unlimited() {
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    module.set_max_connections(0);

    let sessions = (0..4).map(|_| client.session_for(&module)).collect();

    assert_eq!(peak_concurrency(sessions), 4);
}

#[test]
fn modules_are_limited_independently() {
    let client = HttpClient::new().unwrap();
    let a = client.module("a");
    let b = client.module("b");
    a.set_max_connections(1);
    b.set_max_connections(1);

    let sessions = vec![client.session_for(&a), client.session_for(&b)];

    assert_eq!(peak_concurrency(sessions), 2);
}

#[test]
fn the_same_module_id_gives_the_same_queue() {
    let client = HttpClient::new().unwrap();
    client.module("m").set_max_connections(1);

    let sessions = vec![
        client.session_for(&client.module("m")),
        client.session_for(&client.module("m")),
    ];

    assert_eq!(peak_concurrency(sessions), 1);
}
