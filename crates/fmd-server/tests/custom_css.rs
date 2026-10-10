//! `GET /custom.css` from the data folder (docs/tickets/T91-custom-css.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use fmd_server::{AppState, build_router};
use fmd_store::AppDb;
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

/// A state with its data folder in a temp dir.
fn harness() -> (TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let state = AppState::new(db).unwrap().with_data_dir(dir.path());
    (dir, state)
}

async fn get_css(state: &AppState) -> Response {
    let req = Request::get("/custom.css").body(Body::empty()).unwrap();
    build_router(state.clone()).oneshot(req).await.unwrap()
}

async fn body(res: Response) -> String {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn header_of(res: &Response, name: header::HeaderName) -> &str {
    res.headers().get(name).unwrap().to_str().unwrap()
}

/// An empty 200 served as CSS.
async fn assert_empty_stylesheet(res: Response) {
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        header_of(&res, header::CONTENT_TYPE),
        "text/css; charset=utf-8"
    );
    assert_eq!(body(res).await, "");
}

#[tokio::test]
async fn serves_custom_css_from_the_data_folder_as_uncached_css() {
    let (dir, state) = harness();
    std::fs::write(dir.path().join("custom.css"), ":root { --bg: #000; }").unwrap();

    let res = get_css(&state).await;

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        header_of(&res, header::CONTENT_TYPE),
        "text/css; charset=utf-8"
    );
    assert_eq!(header_of(&res, header::CACHE_CONTROL), "no-cache");
    assert_eq!(body(res).await, ":root { --bg: #000; }");
}

#[tokio::test]
async fn an_edit_shows_in_the_next_response() {
    let (dir, state) = harness();
    let file = dir.path().join("custom.css");
    std::fs::write(&file, ":root { --bg: #000; }").unwrap();
    assert_eq!(body(get_css(&state).await).await, ":root { --bg: #000; }");

    std::fs::write(&file, ":root { --bg: #fff; }").unwrap();

    assert_eq!(body(get_css(&state).await).await, ":root { --bg: #fff; }");
}

#[tokio::test]
async fn no_file_is_an_empty_stylesheet() {
    let (_dir, state) = harness();

    assert_empty_stylesheet(get_css(&state).await).await;
}

#[tokio::test]
async fn no_data_folder_is_an_empty_stylesheet() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(AppDb::open(dir.path().join("app.db")).unwrap()).unwrap();

    assert_empty_stylesheet(get_css(&state).await).await;
}

#[tokio::test]
async fn a_file_over_one_mebibyte_is_refused() {
    let (dir, state) = harness();
    std::fs::write(dir.path().join("custom.css"), vec![b' '; 2 * 1024 * 1024]).unwrap();

    assert_eq!(
        get_css(&state).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn needs_no_login_when_a_password_is_set() {
    let (dir, state) = harness();
    let state = state.with_auth("hunter2");
    std::fs::write(dir.path().join("custom.css"), ":root { --bg: #000; }").unwrap();

    let res = get_css(&state).await;

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, ":root { --bg: #000; }");
}
