//! The Komga and Kavita requests built from the `comic_server` settings, and the debounce that
//! turns a batch of finished downloads into one scan (issue #269, "Seams under test").

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fmd_core::comic_server::{ApiRequest, ComicServer, Debounce, HttpMethod};
use fmd_core::settings::{ComicServerKind, ComicServerSettings};

fn settings(kind: ComicServerKind, library_id: Option<&str>) -> ComicServerSettings {
    ComicServerSettings {
        kind,
        url: "http://nas:25600/".into(),
        api_key: Some("key-1".into()),
        library_id: library_id.map(Into::into),
    }
}

fn post(url: &str, header: &str) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Post,
        url: url.into(),
        headers: vec![(header.into(), "key-1".into())],
    }
}

fn get(url: &str, header: &str) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Get,
        ..post(url, header)
    }
}

#[test]
fn komga_with_a_library_id_scans_that_library() {
    let server =
        ComicServer::from_settings(&settings(ComicServerKind::Komga, Some("0B79XX3NP97K9")))
            .unwrap();
    assert_eq!(
        server.scan_requests(&[]),
        vec![post(
            "http://nas:25600/api/v1/libraries/0B79XX3NP97K9/scan",
            "X-API-Key"
        )]
    );
}

#[test]
fn kavita_scans_every_library_with_its_header() {
    let server = ComicServer::from_settings(&settings(ComicServerKind::Kavita, None)).unwrap();
    assert_eq!(server.libraries_request(), None);
    assert_eq!(
        server.scan_requests(&[]),
        vec![post("http://nas:25600/api/Library/scan-all", "x-api-key")]
    );
}

#[test]
fn komga_without_a_library_id_lists_the_libraries_and_scans_each() {
    let server = ComicServer::from_settings(&settings(ComicServerKind::Komga, None)).unwrap();
    assert_eq!(
        server.libraries_request(),
        Some(get("http://nas:25600/api/v1/libraries", "X-API-Key"))
    );
    assert_eq!(
        server.scan_requests(&["A".into(), "B".into()]),
        vec![
            post("http://nas:25600/api/v1/libraries/A/scan", "X-API-Key"),
            post("http://nas:25600/api/v1/libraries/B/scan", "X-API-Key"),
        ]
    );
}

#[test]
fn komga_with_a_library_id_needs_no_listing() {
    let server = ComicServer::from_settings(&settings(ComicServerKind::Komga, Some("A"))).unwrap();
    assert_eq!(server.libraries_request(), None);
}

#[test]
fn the_connection_test_lists_komga_libraries_and_asks_kavita_when_the_key_expires() {
    let komga = ComicServer::from_settings(&settings(ComicServerKind::Komga, Some("A"))).unwrap();
    assert_eq!(
        komga.test_request(),
        get("http://nas:25600/api/v1/libraries", "X-API-Key")
    );
    let kavita = ComicServer::from_settings(&settings(ComicServerKind::Kavita, None)).unwrap();
    assert_eq!(
        kavita.test_request(),
        get("http://nas:25600/api/plugin/authkey-expires", "x-api-key")
    );
}

#[test]
fn no_server_without_a_kind_or_a_url() {
    assert_eq!(
        ComicServer::from_settings(&settings(ComicServerKind::None, None)),
        None
    );
    let blank = ComicServerSettings {
        url: "  ".into(),
        ..settings(ComicServerKind::Kavita, None)
    };
    assert_eq!(ComicServer::from_settings(&blank), None);
}

/// A debounce over 60 s counting its runs.
fn counted() -> (Debounce, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = runs.clone();
    let debounce = Debounce::spawn(Duration::from_secs(60), move || {
        counter.fetch_add(1, Ordering::SeqCst);
        async {}
    });
    (debounce, runs)
}

async fn wait(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

#[tokio::test(start_paused = true)]
async fn finishes_within_the_window_give_one_scan_after_the_last() {
    let (debounce, runs) = counted();
    debounce.trigger();
    wait(30).await;
    debounce.trigger();
    wait(50).await;
    debounce.trigger();
    wait(59).await;
    assert_eq!(
        runs.load(Ordering::SeqCst),
        0,
        "before the window after the last finish"
    );
    wait(2).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    wait(600).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1, "nothing finished since");
}

#[tokio::test(start_paused = true)]
async fn a_finish_after_the_scan_gives_another_scan() {
    let (debounce, runs) = counted();
    debounce.trigger();
    wait(61).await;
    debounce.trigger();
    wait(61).await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}
