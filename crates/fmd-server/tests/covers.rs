//! `GET /api/covers` driven through `build_router` with `oneshot`, fetching from a stub upstream
//! server on a local socket.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use fmd_http::{HttpClient, ModuleHttp};
use fmd_server::{AppState, CoverConfig, CoverModules, CoverSession, build_router, cover_url};
use fmd_store::AppDb;
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

const MODULE_UA: &str = "CoverTest/1.0";

/// Requests the stub upstream received.
#[derive(Clone, Default)]
struct Upstream {
    requests: Arc<Mutex<Vec<HeaderMap>>>,
}

impl Upstream {
    fn hits(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn last(&self) -> HeaderMap {
        self.requests.lock().unwrap().last().unwrap().clone()
    }
}

/// A 400x300 PNG.
fn png() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(400, 300, image::Rgb([200, 30, 30]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

async fn cover(State(up): State<Upstream>, headers: HeaderMap) -> Response {
    up.requests.lock().unwrap().push(headers.clone());
    // Slow enough that concurrent requests overlap.
    tokio::time::sleep(Duration::from_millis(50)).await;
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some("\"v1\"")
    {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::ETAG, "\"v1\""),
        ],
        png(),
    )
        .into_response()
}

/// A page, not an image: must never be served from the app's origin.
async fn page(State(up): State<Upstream>, headers: HeaderMap) -> Response {
    up.requests.lock().unwrap().push(headers);
    (
        [(header::CONTENT_TYPE, "text/html")],
        "<script>alert(1)</script>",
    )
        .into_response()
}

/// The cover on the first request, then a server error.
async fn flaky(State(up): State<Upstream>, headers: HeaderMap) -> Response {
    up.requests.lock().unwrap().push(headers);
    if up.hits() > 1 {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    ([(header::CONTENT_TYPE, "image/png")], png()).into_response()
}

/// A stub site on a local socket; returns its root URL (`http://127.0.0.1:<port>`).
async fn start_upstream(up: Upstream) -> String {
    let app = axum::Router::new()
        .route("/covers/{name}", get(cover))
        .route("/page.html", get(page))
        .route("/flaky/{name}", get(flaky))
        .with_state(up);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Website modules as the composition root would expose them: each with its root URL, shared
/// HTTP state (cookie jar, connection queue) and user agent override.
struct Modules {
    client: HttpClient,
    modules: HashMap<String, (String, ModuleHttp)>,
}

impl CoverModules for Modules {
    fn cover_session(&self, module: &str) -> Option<CoverSession> {
        let (root_url, http) = self.modules.get(module)?;
        let mut session = self.client.session_for(http);
        session.set_user_agent(MODULE_UA);
        Some(CoverSession {
            root_url: root_url.clone(),
            session,
        })
    }
}

fn modules(sites: &[(&str, &str)]) -> Modules {
    let client = HttpClient::new().unwrap();
    let modules = sites
        .iter()
        .map(|(id, root)| {
            let http = client.module(id);
            http.cookies()
                .add_server_cookies(root, "sid=abc123; Path=/", SystemTime::now());
            (id.to_string(), (root.to_string(), http))
        })
        .collect();
    Modules { client, modules }
}

struct Harness {
    _dir: TempDir,
    state: AppState,
}

fn harness(cache: &Path, sites: &[(&str, &str)], config: impl FnOnce(&mut CoverConfig)) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let mut cfg = CoverConfig::new(cache);
    config(&mut cfg);
    Harness {
        _dir: dir,
        state: AppState::new(db).with_covers(cfg, modules(sites)),
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

fn get_req(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn body(res: Response) -> Vec<u8> {
    res.into_body().collect().await.unwrap().to_bytes().to_vec()
}

#[tokio::test(flavor = "multi_thread")]
async fn fetches_with_the_module_http_settings_then_serves_from_disk() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(cache.path(), &[("site", &root)], |_| {});
    let uri = cover_url("site", &format!("{root}/covers/a.png"));

    let res = send(&h.state, get_req(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/png");
    assert!(res.headers().contains_key(header::ETAG));
    assert!(res.headers().contains_key(header::CACHE_CONTROL));
    assert_eq!(res.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(body(res).await, png());

    let seen = up.last();
    assert_eq!(seen[header::USER_AGENT], MODULE_UA);
    assert_eq!(seen[header::REFERER], format!("{root}/").as_str());
    assert!(
        seen[header::COOKIE]
            .to_str()
            .unwrap()
            .contains("sid=abc123"),
        "{seen:?}"
    );

    let res = send(&h.state, get_req(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, png());
    assert_eq!(up.hits(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn if_none_match_from_the_browser_is_a_304() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(cache.path(), &[("site", &root)], |_| {});
    let uri = cover_url("site", &format!("{root}/covers/a.png"));

    let first = send(&h.state, get_req(&uri)).await;
    let etag = first.headers()[header::ETAG].clone();

    let req = Request::get(&uri)
        .header(header::IF_NONE_MATCH, etag.clone())
        .body(Body::empty())
        .unwrap();
    let res = send(&h.state, req).await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(res.headers()[header::ETAG], etag);
    assert!(body(res).await.is_empty());

    let req = Request::get(&uri)
        .header(header::IF_NONE_MATCH, "\"something-else\"")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&h.state, req).await.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests_for_one_cover_make_one_upstream_fetch() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(cache.path(), &[("site", &root)], |_| {});
    let uri = cover_url("site", &format!("{root}/covers/a.png"));

    let requests = (0..10).map(|_| {
        let (state, uri) = (h.state.clone(), uri.clone());
        tokio::spawn(async move { send(&state, get_req(&uri)).await })
    });
    for res in futures_util::future::join_all(requests).await {
        let res = res.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body(res).await, png());
    }
    assert_eq!(up.hits(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn private_network_targets_off_the_module_host_are_rejected() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(
        cache.path(),
        &[("site", &root), ("other", "https://manga.example")],
        |_| {},
    );

    let res = send(
        &h.state,
        get_req(&cover_url("other", &format!("{root}/covers/a.png"))),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    for url in [
        "http://localhost/a.png",
        "http://[::1]/a.png",
        "http://10.0.0.8/a.png",
        "http://192.168.1.1/a.png",
        "http://169.254.169.254/latest/meta-data",
        "ftp://manga.example/a.png",
    ] {
        let res = send(&h.state, get_req(&cover_url("other", url))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{url}");
    }
    assert_eq!(up.hits(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn w_returns_a_thumbnail_of_that_width() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(cache.path(), &[("site", &root)], |_| {});
    let uri = cover_url("site", &format!("{root}/covers/a.png")) + "&w=200";

    let res = send(&h.state, get_req(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let full_etag = send(
        &h.state,
        get_req(&cover_url("site", &format!("{root}/covers/a.png"))),
    )
    .await
    .headers()[header::ETAG]
        .clone();
    assert_ne!(res.headers()[header::ETAG], full_etag);
    let content_type = res.headers()[header::CONTENT_TYPE].clone();
    let thumb = image::load_from_memory(&body(res).await).unwrap();
    assert_eq!((thumb.width(), thumb.height()), (200, 150));
    assert_eq!(content_type, "image/png");

    // Served again from the cache.
    let res = send(&h.state, get_req(&uri)).await;
    let thumb = image::load_from_memory(&body(res).await).unwrap();
    assert_eq!(thumb.width(), 200);
    assert_eq!(up.hits(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cache_survives_a_restart() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let uri = cover_url("site", &format!("{root}/covers/a.png"));

    let before = harness(cache.path(), &[("site", &root)], |_| {});
    assert_eq!(
        send(&before.state, get_req(&uri)).await.status(),
        StatusCode::OK
    );
    drop(before);

    let after = harness(cache.path(), &[("site", &root)], |_| {});
    let res = send(&after.state, get_req(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, png());
    assert_eq!(up.hits(), 1);
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_size_cap_evicts_the_least_recently_used_cover() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    // Room for two covers, not three.
    let cap = png().len() as u64 * 5 / 2;
    let h = harness(cache.path(), &[("site", &root)], |c| c.max_bytes = cap);
    let uri = |name: &str| cover_url("site", &format!("{root}/covers/{name}"));

    send(&h.state, get_req(&uri("a.png"))).await;
    send(&h.state, get_req(&uri("b.png"))).await;
    // Using `a` again makes `b` the least recently used.
    tokio::time::sleep(Duration::from_millis(20)).await;
    send(&h.state, get_req(&uri("a.png"))).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    send(&h.state, get_req(&uri("c.png"))).await;
    assert_eq!(up.hits(), 3);
    assert!(dir_size(cache.path()) <= cap);

    send(&h.state, get_req(&uri("a.png"))).await;
    assert_eq!(up.hits(), 3, "a is still cached");
    send(&h.state, get_req(&uri("b.png"))).await;
    assert_eq!(up.hits(), 4, "b was evicted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_cover_is_revalidated_upstream() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(cache.path(), &[("site", &root)], |c| {
        c.revalidate_after = Duration::ZERO;
    });
    let uri = cover_url("site", &format!("{root}/covers/a.png"));

    let first = send(&h.state, get_req(&uri)).await;
    let etag = first.headers()[header::ETAG].clone();
    let res = send(&h.state, get_req(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::ETAG], etag);
    assert_eq!(body(res).await, png());
    assert_eq!(up.hits(), 2);
    assert_eq!(up.last()[header::IF_NONE_MATCH], "\"v1\"");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_body_that_is_not_an_image_is_refused() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(cache.path(), &[("site", &root)], |_| {});

    let res = send(
        &h.state,
        get_req(&cover_url("site", &format!("{root}/page.html"))),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_cover_is_served_when_upstream_fails() {
    let up = Upstream::default();
    let root = start_upstream(up.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let h = harness(cache.path(), &[("site", &root)], |c| {
        c.revalidate_after = Duration::ZERO;
    });
    let uri = cover_url("site", &format!("{root}/flaky/a.png"));

    assert_eq!(send(&h.state, get_req(&uri)).await.status(), StatusCode::OK);
    let res = send(&h.state, get_req(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, png());
    assert_eq!(up.hits(), 2);
}
