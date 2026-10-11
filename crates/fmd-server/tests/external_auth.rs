//! The "authentication is handled in front of FMD2r" setting through `GET /api/health`
//! (issue #230, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_server::{AppState, build_router};
use fmd_store::AppDb;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn state() -> (TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let state = AppState::new(db)
        .unwrap()
        .with_listen_addr("0.0.0.0:8080".parse().unwrap());
    (dir, state)
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

async fn health(state: &AppState) -> Value {
    let req = Request::get("/api/health").body(Body::empty()).unwrap();
    let res = send(state, req).await;
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn set_external_auth(state: &AppState, on: bool) -> StatusCode {
    let req = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "server": { "external_auth": on } }).to_string(),
        ))
        .unwrap();
    send(state, req).await.status()
}

#[tokio::test]
async fn health_reports_the_external_auth_setting() {
    let (_dir, state) = state();
    assert_eq!(health(&state).await["external_auth"], false);

    assert_eq!(set_external_auth(&state, true).await, StatusCode::OK);
    let body = health(&state).await;
    assert_eq!(body["external_auth"], true);
    // The API itself stays open.
    assert_eq!(body["auth"], false);

    assert_eq!(set_external_auth(&state, false).await, StatusCode::OK);
    assert_eq!(health(&state).await["external_auth"], false);
}

#[tokio::test]
async fn the_command_line_override_wins_over_the_setting_and_is_reported() {
    let (_dir, state) = state();
    set_external_auth(&state, false).await;

    let on = state.clone().with_external_auth(true);
    let body = health(&on).await;
    assert_eq!(body["external_auth"], true);
    assert_eq!(body["overridden"], json!(["server.external_auth"]));

    // Off from the command line beats the setting too.
    set_external_auth(&state, true).await;
    let off = state.clone().with_external_auth(false);
    let body = health(&off).await;
    assert_eq!(body["external_auth"], false);
    assert_eq!(body["overridden"], json!(["server.external_auth"]));
}
