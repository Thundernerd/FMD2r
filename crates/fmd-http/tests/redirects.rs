// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use common::{TestServer, echo, echoed_header};
use fmd_http::HttpClient;

/// `/chain/{n}/{len}` redirects (302) to `/chain/{n+1}/{len}` until `n == len`, then echoes.
async fn chain(
    State(hits): State<Arc<AtomicUsize>>,
    Path((n, len)): Path<(usize, usize)>,
    method: axum::http::Method,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    hits.fetch_add(1, Ordering::SeqCst);
    if n < len {
        (
            StatusCode::FOUND,
            [(header::LOCATION, format!("/chain/{}/{len}", n + 1))],
            "moved",
        )
            .into_response()
    } else {
        echo(method, headers, body).await.into_response()
    }
}

fn server() -> (TestServer, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let router = Router::new()
        .route("/chain/{n}/{len}", any(chain))
        .route("/echo", any(echo))
        .route(
            "/see-other",
            any(|| async { (StatusCode::SEE_OTHER, [(header::LOCATION, "/echo")]) }),
        )
        .route(
            "/permanent",
            any(|| async {
                (
                    StatusCode::PERMANENT_REDIRECT,
                    [(header::LOCATION, "/echo")],
                    "308 body",
                )
            }),
        )
        .with_state(hits.clone());
    (TestServer::start(router), hits)
}

#[test]
fn five_redirects_are_followed() {
    let (server, hits) = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    assert!(session.get(&server.url("/chain/0/5")).unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 6);
    assert_eq!(session.result_code(), 200);
    assert_eq!(session.last_url(), server.url("/chain/5/5"));
}

#[test]
fn a_sixth_redirect_stops_the_request_and_returns_false() {
    let (server, hits) = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    assert!(!session.get(&server.url("/chain/0/6")).unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 6);
    assert_eq!(session.result_code(), 302);
}

#[test]
fn see_other_after_post_becomes_a_get_without_body() {
    let (server, _) = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    assert!(session.post(&server.url("/see-other"), b"a=1").unwrap());

    let doc = session.document();
    assert!(
        doc.starts_with(b"GET\n"),
        "{}",
        String::from_utf8_lossy(doc)
    );
    assert!(doc.ends_with(b"\n\n"));
    assert_eq!(echoed_header(doc, "content-type"), None);
}

#[test]
fn redirects_add_the_original_url_as_referer() {
    let (server, _) = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    session.get(&server.url("/chain/0/2")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "referer"),
        Some(server.url("/chain/0/2"))
    );
}

#[test]
fn an_explicit_referer_is_kept_across_redirects() {
    let (server, _) = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    session
        .headers_mut()
        .set_value("Referer", "https://elsewhere.test/");

    session.get(&server.url("/chain/0/1")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "referer").as_deref(),
        Some("https://elsewhere.test/")
    );
}

#[test]
fn permanent_redirect_308_is_not_followed() {
    let (server, _) = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    assert!(session.get(&server.url("/permanent")).unwrap());

    assert_eq!(session.result_code(), 308);
    assert_eq!(session.document(), b"308 body");
}
