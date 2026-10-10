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
        state: AppState::new(db.clone()).unwrap(),
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
async fn inbox_shows_module_updater_reports_as_plain_text() {
    let h = harness();
    let report = |severity, title: &str, body| NewEvent {
        kind: "module_update".into(),
        severity,
        module_id: None,
        task_id: None,
        title: title.into(),
        body,
    };
    h.db.events()
        .push(&report(
            EventSeverity::Warning,
            "module OrckuMangas.lua uses unknown Host API names",
            serde_json::json!({
                "file": "modules/OrckuMangas.lua",
                "names": ["MANGAINFO.Artist", "MANGAINFO.Foo"],
            }),
        ))
        .unwrap();
    h.db.events()
        .push(&report(
            EventSeverity::Error,
            "module Broken.lua failed Init",
            serde_json::json!({
                "file": "modules/Broken.lua",
                "error": "modules/Broken.lua:3: boom\nstack traceback:\n\t[C]: in ?",
            }),
        ))
        .unwrap();

    let items = body_json(send(&h.state, get("/api/inbox")).await).await;
    let body = |title: &str| {
        items
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["title"] == title)
            .unwrap()["body"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        body("module OrckuMangas.lua uses unknown Host API names"),
        "Unknown Host API names: MANGAINFO.Artist, MANGAINFO.Foo"
    );
    assert_eq!(
        body("module Broken.lua failed Init"),
        "modules/Broken.lua:3: boom\nstack traceback:\n\t[C]: in ?"
    );
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
        "/api/jobs",
        "/api/jobs/{id}",
        "/api/jobs/{id}/run",
        "/api/modules/update",
        "/api/jobs/{id}/cancel",
        "/api/about",
        "/api/settings",
        "/api/covers",
        "/api/preview-rename",
        "/api/modules",
        "/api/modules/{id}/settings",
    ] {
        assert!(paths.contains_key(path), "missing {path}");
    }
    assert!(paths["/api/settings"]["patch"].is_object());
    assert!(paths["/api/modules/{id}/settings"]["patch"].is_object());
}

#[test]
fn exported_openapi_json_matches_the_served_document() {
    let exported: serde_json::Value =
        serde_json::from_str(&fmd_server::openapi_json().unwrap()).unwrap();
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
    let logs = LogBuffer::new(100, EventBus::new());
    let state = h.state.clone().with_logs(logs.clone());
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
    assert_eq!(lines[1]["message"], "Init failed");
    assert_eq!(lines[1]["module"], "mangadex");

    let since = lines[0]["seq"].as_u64().unwrap();
    let tail = body_json(send(&state, get(&format!("/api/logs?since={since}"))).await).await;
    assert_eq!(tail.as_array().unwrap().len(), 1);
    assert_eq!(tail[0]["message"], "Init failed");
}

#[tokio::test]
async fn log_buffer_keeps_only_the_newest_lines_and_streams_them() {
    let h = harness();
    let logs = LogBuffer::new(2, EventBus::new());
    let state = h.state.clone().with_logs(logs.clone());
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
async fn settings_are_typed_with_fmd2_defaults_and_merge_patched() {
    let h = harness();
    let res = send(&h.state, get("/api/settings")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    // `OptionMaxParallel` = 1 and `OptionConnectionTimeout` = 30 (baseunits/FMDOptions.pas:129-133).
    assert_eq!(body["connections"]["max_parallel_tasks"], 1);
    assert_eq!(body["connections"]["timeout_secs"], 30);

    let patch = serde_json::json!({ "connections": { "max_parallel_tasks": 4 } });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await["connections"]["max_parallel_tasks"], 4);
    let body = body_json(send(&h.state, get("/api/settings")).await).await;
    assert_eq!(body["connections"]["max_parallel_tasks"], 4);
    assert_eq!(body["connections"]["timeout_secs"], 30);

    let patch = serde_json::json!({ "connections": { "max_parallel_tasks": null } });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    assert_eq!(body_json(res).await["connections"]["max_parallel_tasks"], 1);
}

#[tokio::test]
async fn an_invalid_setting_is_a_422_naming_the_field_and_changes_nothing() {
    let h = harness();
    // The timeout spin edit spans 1..=300 (mangadownloader/forms/frmMain.lfm:3387-3388).
    let patch = serde_json::json!({
        "connections": { "timeout_secs": 0, "max_parallel_tasks": 3 }
    });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
    let problem = body_json(res).await;
    assert_eq!(problem["field"], "connections.timeout_secs");
    assert_eq!(problem["status"], 422);

    let res = send(
        &h.state,
        patch_json(
            "/api/settings",
            serde_json::json!({ "connections": { "nope": 1 } }),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(res).await["field"], "connections.nope");

    let body = body_json(send(&h.state, get("/api/settings")).await).await;
    assert_eq!(body["connections"]["max_parallel_tasks"], 1);
}

#[tokio::test]
async fn every_invalid_setting_of_a_patch_is_reported_in_one_422() {
    let h = harness();
    // Out of range (timeout 1..=300, mangadownloader/forms/frmMain.lfm:3387-3388), the wrong
    // type, and unknown: all three come back, in patch order, and nothing is stored.
    let patch = serde_json::json!({
        "connections": { "timeout_secs": 0, "max_parallel_tasks": "four", "nope": 1 },
        "general": { "language": "nl" },
    });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let problem = body_json(res).await;
    let mut fields: Vec<&str> = problem["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["field"].as_str().unwrap())
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "connections.max_parallel_tasks",
            "connections.nope",
            "connections.timeout_secs"
        ]
    );
    assert!(
        problem["fields"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| !f["detail"].as_str().unwrap().is_empty())
    );
    // `field` stays for clients that read one.
    assert!(fields.contains(&problem["field"].as_str().unwrap()));

    let body = body_json(send(&h.state, get("/api/settings")).await).await;
    assert_eq!(body["general"]["language"], "en");
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

#[tokio::test]
async fn wrong_method_on_an_api_route_is_a_405_problem() {
    let h = harness();
    let res = send(
        &h.state,
        Request::delete("/api/inbox").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
}

#[tokio::test]
async fn missing_asset_files_are_404_not_index_html() {
    let h = harness();
    let state = h.state.clone().with_assets(built_web());
    let res = send(&state, get("/_app/missing.js")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bearer_scheme_is_case_insensitive_and_401_names_it() {
    let h = authed_harness();
    let res = send(&h.state, get("/api/inbox")).await;
    assert_eq!(res.headers()["www-authenticate"], "Bearer");
    let req = Request::get("/api/inbox")
        .header("authorization", format!("bearer {SECRET}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&h.state, req).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn debug_lines_are_buffered_but_not_streamed() {
    let h = harness();
    let logs = LogBuffer::new(10, EventBus::new());
    let state = h.state.clone().with_logs(logs.clone());
    let res = send(&state, get("/api/events")).await;
    emit_logs(&logs, || {
        tracing::debug!("chatty");
        tracing::info!("worth streaming");
    });
    let lines = body_json(send(&state, get("/api/logs")).await).await;
    assert_eq!(lines.as_array().unwrap().len(), 2);
    let seen = read_sse_until(res, "worth streaming").await;
    assert!(!seen.contains("chatty"), "{seen}");
}

fn preview(body: serde_json::Value) -> Request<Body> {
    Request::post("/api/preview-rename")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn rename_templates_are_previewed_on_sample_values() {
    let h = harness();
    let draft = serde_json::json!({ "saveto": {
        "manga_rename": "%WEBSITE% - %MANGA%",
        "chapter_rename": "%NUMBERING% %CHAPTER%",
        "filename_rename": "page %FILENAME%",
        "remove_manga_name_from_chapter": true,
        "convert_digit_volume": true,
        "digit_volume_length": 2,
        "convert_digit_chapter": true,
        "digit_chapter_length": 3,
    }});
    let res = send(&h.state, preview(draft)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    // CustomRename (baseunits/uBaseUnit.pas:1798-1859) with VolumeChapterPadZero
    // (baseunits/uMisc.pas:119-259) on the sample chapter "Sample Manga - Vol. 1 Ch. 5" stripped
    // of the title (baseunits/uData.pas:186-200), numbering "0005"; a page's file name is the
    // template with `%FILENAME%` as the 1-based page number padded to 3
    // (baseunits/uDownloadsManager.pas:530-552), and the sample page is a JPEG.
    assert_eq!(body["manga"], "MangaDex - Sample Manga");
    assert_eq!(body["chapter"], "0005 Vol. 01 Ch. 005");
    assert_eq!(body["filename"], "page 001");
    assert_eq!(body["page"], "page 001.jpg");
    assert_eq!(
        body["path"],
        "downloads/MangaDex - Sample Manga/0005 Vol. 01 Ch. 005/page 001.jpg"
    );
}

#[tokio::test]
async fn the_preview_keeps_the_title_in_chapter_names_unless_told_to_remove_it() {
    let h = harness();
    let saveto = |remove: bool| {
        serde_json::json!({ "saveto": {
            "chapter_rename": "%CHAPTER%",
            "remove_manga_name_from_chapter": remove,
            "convert_digit_volume": false,
            "convert_digit_chapter": false,
        }})
    };
    // `OptionRemoveMangaNameFromChapter` (baseunits/uData.pas:186-200).
    let body = body_json(send(&h.state, preview(saveto(false))).await).await;
    assert_eq!(body["chapter"], "Sample Manga - Vol. 1 Ch. 5");
    let body = body_json(send(&h.state, preview(saveto(true))).await).await;
    assert_eq!(body["chapter"], "Vol. 1 Ch. 5");
}

#[tokio::test]
async fn the_preview_path_is_where_the_engine_writes_the_first_page() {
    let h = harness();
    // No chapter folder: the pages go straight into the manga folder
    // (baseunits/uDownloadsManager.pas:1143-1152); ImageMagick saves them as its format
    // (baseunits/uDownloadsManager.pas:613-711).
    let draft = serde_json::json!({
        "saveto": { "generate_chapter_folder": false, "convert_digit_volume": false,
            "convert_digit_chapter": false },
        "images": { "imagemagick": { "enabled": true, "save_as": "PNG" } },
    });
    let body = body_json(send(&h.state, preview(draft)).await).await;
    assert_eq!(body["page"], "001.png");
    assert_eq!(body["path"], "downloads/Sample Manga/001.png");

    // Packed: the chapter becomes `<save to>/<chapter name>.cbz` holding the pages
    // (baseunits/uDownloadsManager.pas:553-611).
    let draft = serde_json::json!({
        "saveto": { "generate_manga_folder": false, "convert_digit_volume": false,
            "convert_digit_chapter": false, "default_dir": "/data" },
        "output": { "format": "cbz" },
    });
    let body = body_json(send(&h.state, preview(draft)).await).await;
    assert_eq!(body["page"], "001.jpg");
    assert_eq!(body["path"], "/data/Sample Manga - Vol. 1 Ch. 5.cbz");
}

/// Patches every settings secret, each to a value that appears nowhere else in a response.
fn set_every_secret() -> Request<Body> {
    patch_json(
        "/api/settings",
        serde_json::json!({
            "connections": { "proxy": { "password": "proxy-secret" } },
            "module_updater": { "github_token": "token-secret" },
            "server": { "auth_token": "server-secret" },
        }),
    )
}

/// `req` with the server password [`set_every_secret`] sets as its bearer token: it applies at
/// once.
fn authed(mut req: Request<Body>) -> Request<Body> {
    let bearer = axum::http::HeaderValue::from_static("Bearer server-secret");
    req.headers_mut().insert("authorization", bearer);
    req
}

/// Asserts that `body` holds none of the values [`set_every_secret`] stores.
fn assert_no_secret(body: &serde_json::Value) {
    let text = body.to_string();
    for secret in ["proxy-secret", "token-secret", "server-secret"] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
    assert!(body["connections"]["proxy"].get("password").is_none());
    assert!(body["module_updater"].get("github_token").is_none());
    assert!(body["server"].get("auth_token").is_none());
}

#[tokio::test]
async fn settings_secrets_are_never_returned_only_whether_they_are_set() {
    let h = harness();
    let body = body_json(send(&h.state, get("/api/settings")).await).await;
    assert_no_secret(&body);
    assert_eq!(body["connections"]["proxy"]["has_password"], false);
    assert_eq!(body["module_updater"]["has_github_token"], false);
    assert_eq!(body["server"]["has_auth_token"], false);

    let res = send(&h.state, set_every_secret()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_no_secret(&body_json(res).await);

    let body = body_json(send(&h.state, authed(get("/api/settings"))).await).await;
    assert_no_secret(&body);
    assert_eq!(body["connections"]["proxy"]["has_password"], true);
    assert_eq!(body["module_updater"]["has_github_token"], true);
    assert_eq!(body["server"]["has_auth_token"], true);
}

#[tokio::test]
async fn a_patch_without_a_secret_keeps_it_and_an_empty_one_clears_it() {
    let h = harness();
    send(&h.state, set_every_secret()).await;

    let patch = serde_json::json!({ "connections": { "proxy": { "host": "proxy.example" } } });
    let res = send(&h.state, authed(patch_json("/api/settings", patch))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(send(&h.state, authed(get("/api/settings"))).await).await;
    assert_eq!(body["connections"]["proxy"]["has_password"], true);
    assert_eq!(body["module_updater"]["has_github_token"], true);
    assert_eq!(body["server"]["has_auth_token"], true);

    let clear = serde_json::json!({
        "connections": { "proxy": { "password": "" } },
        "module_updater": { "github_token": "" },
        "server": { "auth_token": "" },
    });
    let res = send(&h.state, authed(patch_json("/api/settings", clear))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(send(&h.state, get("/api/settings")).await).await;
    assert_eq!(body["connections"]["proxy"]["has_password"], false);
    assert_eq!(body["module_updater"]["has_github_token"], false);
    assert_eq!(body["server"]["has_auth_token"], false);
}

#[tokio::test]
async fn settings_report_and_set_whether_setup_is_completed() {
    let h = harness();
    let body = body_json(send(&h.state, get("/api/settings")).await).await;
    assert_eq!(body["general"]["setup_completed"], false);
    assert_eq!(body["general"]["setup_step"], "");

    let patch = serde_json::json!({ "general": { "setup_step": "format" } });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let patch = serde_json::json!({ "general": { "setup_completed": true } });
    let res = send(&h.state, patch_json("/api/settings", patch)).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_json(send(&h.state, get("/api/settings")).await).await;
    assert_eq!(body["general"]["setup_completed"], true);
    assert_eq!(body["general"]["setup_step"], "format");
}
