//! HTTP handlers driven through `build_router` with `oneshot` (no socket) and a temp store.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_server::{
    AppState, EventBus, LogBuffer, ServerEvent, TaskProgress, TaskState, build_router,
};
use fmd_store::{AppDb, EventSeverity, NewEvent};
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

struct Harness {
    _dir: TempDir,
    db: AppDb,
    state: AppState,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    Harness {
        _dir: dir,
        state: AppState::new(db.clone()),
        db,
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn body_json(res: Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_is_ok() {
    let h = harness();
    let res = send(&h.state, get("/api/health")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await["status"], "ok");
}

#[tokio::test]
async fn unknown_api_path_is_a_json_404() {
    let h = harness();
    let res = send(&h.state, get("/api/unknown")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
    let body = body_json(res).await;
    assert_eq!(body["status"], 404);
    assert_eq!(body["title"], "Not Found");
}

fn built_web() -> HashMap<String, Vec<u8>> {
    HashMap::from([
        ("index.html".into(), b"<html>fmd2r spa</html>".to_vec()),
        ("_app/start.js".into(), b"console.log(1)".to_vec()),
    ])
}

async fn body_text(res: Response) -> String {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn spa_routes_fall_back_to_index_html() {
    let h = harness();
    let state = h.state.clone().with_assets(built_web());
    let res = send(&state, get("/some/spa/route")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(body_text(res).await, "<html>fmd2r spa</html>");
}

#[tokio::test]
async fn built_assets_are_served_with_their_mime_type() {
    let h = harness();
    let state = h.state.clone().with_assets(built_web());
    let res = send(&state, get("/_app/start.js")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/javascript");
    assert_eq!(body_text(res).await, "console.log(1)");
}

#[tokio::test]
async fn without_a_built_web_ui_a_placeholder_page_is_served() {
    let h = harness();
    let state = h.state.clone().with_assets(HashMap::new());
    let res = send(&state, get("/")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body_text(res).await.contains("web UI has not been built"));
}

fn new_chapters_event() -> NewEvent {
    NewEvent {
        kind: "new_chapters".into(),
        severity: EventSeverity::Warning,
        module_id: Some("mangadex".into()),
        task_id: None,
        title: "2 new chapters".into(),
        body: serde_json::json!("One Piece: Ch. 1100-1101"),
    }
}

fn post(uri: &str) -> Request<Body> {
    Request::post(uri).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn inbox_lists_stored_events_and_marks_them_read() {
    let h = harness();
    let event = h.db.events().push(&new_chapters_event()).unwrap();

    let res = send(&h.state, get("/api/inbox")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let items = body_json(res).await;
    assert_eq!(items.as_array().unwrap().len(), 1);
    let item = &items[0];
    assert_eq!(item["id"], event.id.0.to_string());
    assert_eq!(item["kind"], "warn");
    assert_eq!(item["title"], "2 new chapters");
    assert_eq!(item["body"], "One Piece: Ch. 1100-1101");
    assert_eq!(item["read"], false);

    let res = send(&h.state, post(&format!("/api/inbox/{}/read", event.id.0))).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let items = body_json(send(&h.state, get("/api/inbox")).await).await;
    assert_eq!(items[0]["read"], true);
}

#[tokio::test]
async fn marking_an_unknown_inbox_item_read_is_a_404() {
    let h = harness();
    let res = send(&h.state, post("/api/inbox/42/read")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = send(&h.state, post("/api/inbox/nope/read")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

const SECRET: &str = "hunter2";

fn authed_harness() -> Harness {
    let mut h = harness();
    h.state = h.state.with_auth(SECRET);
    h
}

fn login(password: &str) -> Request<Body> {
    Request::post("/api/login")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "password": password }).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn with_auth_configured_api_requires_a_token() {
    let h = authed_harness();
    let res = send(&h.state, get("/api/inbox")).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(res).await["status"], 401);

    let wrong = Request::get("/api/inbox")
        .header("authorization", "Bearer hunter3")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&h.state, wrong).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let right = Request::get("/api/inbox")
        .header("authorization", format!("Bearer {SECRET}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&h.state, right).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn with_auth_configured_health_stays_public() {
    let h = authed_harness();
    assert_eq!(
        send(&h.state, get("/api/health")).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn login_with_the_wrong_password_is_rejected() {
    let h = authed_harness();
    let res = send(&h.state, login("wrong")).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(res.headers().get("set-cookie").is_none());
}

#[tokio::test]
async fn login_sets_a_session_cookie_that_authorizes_api_calls() {
    let h = authed_harness();
    let res = send(&h.state, login(SECRET)).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let cookie = res.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("HttpOnly"));
    let pair = cookie.split(';').next().unwrap().to_string();

    let req = Request::get("/api/inbox")
        .header("cookie", pair)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&h.state, req).await.status(), StatusCode::OK);

    let forged = Request::get("/api/inbox")
        .header("cookie", "fmd2r_session=forged")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&h.state, forged).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn openapi_document_is_3_1_and_lists_the_api_paths() {
    let h = authed_harness();
    let res = send(&h.state, get("/api/openapi.json")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let doc = body_json(res).await;
    assert!(doc["openapi"].as_str().unwrap().starts_with("3.1"));
    let paths = doc["paths"].as_object().unwrap();
    for path in [
        "/api/health",
        "/api/login",
        "/api/inbox",
        "/api/inbox/{id}/read",
        "/api/events",
        "/api/logs",
        "/api/settings",
    ] {
        assert!(paths.contains_key(path), "missing {path}");
    }
    assert!(paths["/api/settings"]["patch"].is_object());
}

#[test]
fn exported_openapi_json_matches_the_served_document() {
    let exported: serde_json::Value = serde_json::from_str(&fmd_server::openapi_json()).unwrap();
    assert_eq!(exported["info"]["title"], "FMD2r");
    assert!(exported["paths"]["/api/inbox"]["get"].is_object());
}

/// Reads SSE body chunks until `needle` shows up (or times out), returning everything read.
async fn read_sse_until(res: Response, needle: &str) -> String {
    let mut body = res.into_body();
    let mut seen = String::new();
    let read = async {
        while !seen.contains(needle) {
            let Some(frame) = body.frame().await else {
                break;
            };
            if let Ok(data) = frame.unwrap().into_data() {
                seen.push_str(std::str::from_utf8(&data).unwrap());
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), read)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {needle:?}; got {seen:?}"));
    seen
}

#[tokio::test]
async fn events_stream_delivers_published_events_as_sse_frames() {
    let h = harness();
    let res = send(&h.state, get("/api/events")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/event-stream");

    h.state
        .events()
        .publish(ServerEvent::TaskProgress(TaskProgress {
            id: 7,
            title: "One Piece".into(),
            chapters: "Ch. 1100".into(),
            status: TaskState::Downloading,
            done: 3,
            total: 20,
            bytes_per_sec: 1024.0,
        }));

    let seen = read_sse_until(res, "\n\n").await;
    assert!(seen.contains("event: task.progress\n"), "{seen}");
    let data = seen.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
    let data: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(data["id"], 7);
    assert_eq!(data["status"], "downloading");
    assert_eq!(data["done"], 3);
}

#[tokio::test]
async fn events_stream_replays_inbox_events_after_last_event_id() {
    let h = harness();
    let first = h.db.events().push(&new_chapters_event()).unwrap();
    let mut second = new_chapters_event();
    second.title = "module update failed".into();
    let second = h.db.events().push(&second).unwrap();

    let req = Request::get("/api/events")
        .header("last-event-id", first.id.0.to_string())
        .body(Body::empty())
        .unwrap();
    let seen = read_sse_until(send(&h.state, req).await, "\n\n").await;
    assert!(seen.contains("event: inbox.new\n"), "{seen}");
    assert!(seen.contains(&format!("id: {}\n", second.id.0)), "{seen}");
    assert!(seen.contains("module update failed"), "{seen}");
    assert!(!seen.contains("2 new chapters"), "{seen}");
}

#[tokio::test]
async fn events_stream_requires_auth_when_configured() {
    let h = authed_harness();
    let res = send(&h.state, get("/api/events")).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn notifying_stores_an_inbox_item_and_streams_it() {
    let h = harness();
    let res = send(&h.state, get("/api/events")).await;
    let item = h.state.notify(new_chapters_event()).await.unwrap();

    let seen = read_sse_until(res, "\n\n").await;
    assert!(seen.contains("event: inbox.new\n"), "{seen}");
    assert!(seen.contains(&format!("id: {}\n", item.id)), "{seen}");
    let items = body_json(send(&h.state, get("/api/inbox")).await).await;
    assert_eq!(items[0]["id"], item.id);
}

fn emit_logs(logs: &LogBuffer, emit: impl FnOnce()) {
    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry().with(logs.clone());
    tracing::subscriber::with_default(subscriber, emit);
}

#[tokio::test]
async fn logs_returns_the_tail_of_traced_lines_since_a_sequence_number() {
    let h = harness();
    let bus = EventBus::new();
    let logs = LogBuffer::new(100, bus.clone());
    let state = h.state.clone().with_event_bus(bus).with_logs(logs.clone());
    emit_logs(&logs, || {
        tracing::info!(target: "fmd_core::jobs", "favorites check started");
        tracing::warn!(target: "fmd_lua", module = "mangadex", "Init failed");
    });

    let lines = body_json(send(&state, get("/api/logs")).await).await;
    let lines = lines.as_array().unwrap().clone();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["level"], "INFO");
    assert_eq!(lines[0]["target"], "fmd_core::jobs");
    assert_eq!(lines[0]["message"], "favorites check started");
    assert_eq!(lines[1]["level"], "WARN");
    assert_eq!(lines[1]["message"], "Init failed module=mangadex");

    let since = lines[0]["seq"].as_u64().unwrap();
    let tail = body_json(send(&state, get(&format!("/api/logs?since={since}"))).await).await;
    assert_eq!(tail.as_array().unwrap().len(), 1);
    assert_eq!(tail[0]["message"], "Init failed module=mangadex");
}

#[tokio::test]
async fn log_buffer_keeps_only_the_newest_lines_and_streams_them() {
    let h = harness();
    let bus = EventBus::new();
    let logs = LogBuffer::new(2, bus.clone());
    let state = h.state.clone().with_event_bus(bus).with_logs(logs.clone());
    let res = send(&state, get("/api/events")).await;
    emit_logs(&logs, || {
        for i in 0..3 {
            tracing::info!("line {i}");
        }
    });

    let lines = body_json(send(&state, get("/api/logs")).await).await;
    let messages: Vec<_> = lines
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["message"].clone())
        .collect();
    assert_eq!(messages, ["line 1", "line 2"]);

    let seen = read_sse_until(res, "line 0").await;
    assert!(seen.contains("event: log\n"), "{seen}");
}

fn patch_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::patch(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn settings_are_read_and_merge_patched() {
    let h = harness();
    let res = send(&h.state, get("/api/settings")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await, serde_json::json!({}));

    let patch =
        serde_json::json!({ "downloads": { "max_tasks": 2, "dir": "/manga" }, "theme": "dark" });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    assert_eq!(res.status(), StatusCode::OK);

    let patch = serde_json::json!({ "downloads": { "max_tasks": 4 }, "theme": null });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    let expected = serde_json::json!({ "downloads": { "max_tasks": 4, "dir": "/manga" } });
    assert_eq!(body_json(res).await, expected);
    assert_eq!(
        body_json(send(&h.state, get("/api/settings")).await).await,
        expected
    );
}

#[tokio::test]
async fn patching_settings_with_a_non_object_is_a_400_problem() {
    let h = harness();
    let res = send(
        &h.state,
        patch_json("/api/settings", serde_json::json!([1, 2])),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(res).await["status"], 400);
}

#[tokio::test]
async fn malformed_request_bodies_are_400_problems() {
    let h = harness();
    let req = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from("{not json"))
        .unwrap();
    let res = send(&h.state, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(res.headers()["content-type"], "application/problem+json");

    let res = send(&h.state, get("/api/logs?since=yesterday")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
}
