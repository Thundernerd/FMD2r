//! Asking Komga or Kavita to scan after downloads finish: the real download engine with a
//! fixture module, and a local HTTP stub posing as the comic server (issue #269, "Seams under
//! test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request as StubRequest, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use fmd_core::download::{DownloadManager, EngineConfig};
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_server::{AppState, build_router};
use fmd_store::{AppDb, NewFavorite};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// How long a test waits for the engine before failing.
const PATIENCE: Duration = Duration::from_secs(20);

/// The scan delay the tests use instead of 60 s.
const WINDOW: Duration = Duration::from_secs(1);

/// Serves a PNG for every URL holding `/img/` but not `/missing/`, a 404 otherwise.
struct Pages;

impl Transport for Pages {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let found = request.url.contains("/img/") && !request.url.contains("/missing/");
        let response = WireResponse {
            status: if found { 200 } else { 404 },
            reason: String::new(),
            headers: Vec::new(),
            body: if found { PNG.to_vec() } else { Vec::new() },
        };
        Box::pin(async move { Ok(response) })
    }
}

/// A module whose chapter `/c/<n>` has `n` pages at `https://t/img/c/<n>/<page>`.
const T: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='t'; m.Name='T'; m.RootURL='https://t'; m.OnGetPageNumber='GPN' end
function GPN()
  for i = 1, tonumber(URL:match('%d+$')) do TASK.PageLinks.Add('https://t/img' .. URL .. '/' .. i) end
  return true
end
"#;

/// One request the stub comic server received: method, path and API key header.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    method: String,
    path: String,
    key: Option<String>,
}

fn seen(method: &str, path: &str, key: &str) -> Seen {
    Seen {
        method: method.into(),
        path: path.into(),
        key: Some(key.into()),
    }
}

#[derive(Clone, Default)]
struct Stub {
    seen: Arc<Mutex<Vec<Seen>>>,
    /// Answer 401 to everything.
    unauthorized: bool,
    /// Answer 500 to this path.
    broken: Option<&'static str>,
}

/// Komga's and Kavita's endpoints, answering every scan with 202 and Komga's libraries with
/// `L1` and `L2`.
async fn stub_handler(State(stub): State<Stub>, headers: HeaderMap, req: StubRequest) -> Response {
    let key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let path = req.uri().path().to_owned();
    stub.seen.lock().unwrap().push(Seen {
        method: req.method().to_string(),
        path: path.clone(),
        key,
    });
    if stub.unauthorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if stub.broken == Some(path.as_str()) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    match path.as_str() {
        "/api/v1/libraries" => axum::Json(json!([
            {"id": "L1", "name": "Manga"},
            {"id": "L2", "name": "Comics"},
        ]))
        .into_response(),
        "/api/plugin/authkey-expires" => axum::Json(json!("2027-01-01T00:00:00Z")).into_response(),
        _ => StatusCode::ACCEPTED.into_response(),
    }
}

/// Starts the stub on a local socket; returns its root URL (`http://127.0.0.1:<port>`).
async fn start_stub(stub: Stub) -> String {
    let app = Router::new().fallback(stub_handler).with_state(stub);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

struct Harness {
    dir: TempDir,
    db: AppDb,
    state: AppState,
    stub: Stub,
}

/// The engine with comic server scans after [`WINDOW`], and the stub set up as `comic_server`
/// (merged into a Komga server scanning library `L1` with key `key-1`).
async fn harness(stub: Stub, comic_server: Value) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let lua = dir.path().join("lua");
    fs::create_dir_all(lua.join("modules")).unwrap();
    fs::write(lua.join("modules/T.lua"), T).unwrap();
    let report = ModuleRegistry::load_dir(&lua);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let http = HttpClient::with_transport(Arc::new(Pages)).unwrap();
    let mut config = PoolConfig::new(http.clone());
    config.threads = 2;
    config.lua_dir = lua;
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let settings = Arc::new(SettingsService::load(db.clone()).unwrap());
    let url = start_stub(stub.clone()).await;
    let mut server = json!({"kind": "komga", "url": url, "api_key": "key-1", "library_id": "L1"});
    merge(&mut server, comic_server);
    settings
        .update(json!({
            "output": {"format": "cbz"},
            "saveto": {"default_dir": dir.path().join("out").to_string_lossy()},
            "connections": {"retry_count": 0, "auto_retry_failed_tasks": 0},
            "comic_server": server,
        }))
        .unwrap();
    let engine = DownloadManager::open(EngineConfig::new(
        db.clone(),
        Arc::new(WorkerPool::new(config).unwrap()),
        Arc::new(report.registry),
        settings.clone(),
        http,
    ))
    .await
    .unwrap();
    let state = AppState::new(db.clone())
        .unwrap()
        .with_settings(settings)
        .with_engine(engine);
    state.start_comic_server_scans(WINDOW);
    Harness {
        dir,
        db,
        state,
        stub,
    }
}

fn merge(target: &mut Value, patch: Value) {
    if let (Value::Object(target), Value::Object(patch)) = (target, patch) {
        target.extend(patch);
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn post(uri: &str, body: Option<Value>) -> Request<Body> {
    let req = Request::post(uri).header("content-type", "application/json");
    req.body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap()
}

async fn body_json(res: Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Queues chapter `link` of the manga "Manga" and answers the new task's ID.
async fn add(state: &AppState, link: &str) -> i64 {
    let body = json!({"module_id": "t", "link": "/manga", "title": "Manga",
        "chapters": [{"name": "Ch", "link": link}]});
    let res = send(state, post("/api/tasks", Some(body))).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    body_json(res).await["id"].as_i64().unwrap()
}

async fn wait_for(state: &AppState, id: i64, status: &str) {
    let mut last = Value::Null;
    let wait = async {
        loop {
            let res = send(state, get(&format!("/api/tasks/{id}"))).await;
            last = body_json(res).await;
            if last["task"]["status"] == status {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    if tokio::time::timeout(PATIENCE, wait).await.is_err() {
        panic!("task {id} did not reach {status}; last {last}");
    }
}

/// Waits out the scan delay and then some.
async fn after_the_window() {
    tokio::time::sleep(WINDOW * 2).await;
}

fn requests(h: &Harness) -> Vec<Seen> {
    h.stub.seen.lock().unwrap().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn finished_downloads_lead_to_one_scan_after_the_window() {
    let h = harness(Stub::default(), json!({})).await;
    let first = add(&h.state, "/c/1").await;
    let second = add(&h.state, "/c/2").await;
    wait_for(&h.state, first, "finished").await;
    wait_for(&h.state, second, "finished").await;
    assert_eq!(requests(&h), [], "not before the window");

    after_the_window().await;
    assert_eq!(
        requests(&h),
        [seen("POST", "/api/v1/libraries/L1/scan", "key-1")]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_download_leads_to_no_scan() {
    let h = harness(Stub::default(), json!({})).await;
    let id = add(&h.state, "/missing/1").await;
    wait_for(&h.state, id, "failed").await;

    after_the_window().await;
    assert_eq!(requests(&h), []);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_answering_401_gives_a_warning_in_the_inbox_and_the_download_stays_finished() {
    let stub = Stub {
        unauthorized: true,
        ..Stub::default()
    };
    let h = harness(stub, json!({})).await;
    let id = add(&h.state, "/c/1").await;
    wait_for(&h.state, id, "finished").await;

    after_the_window().await;
    assert_eq!(requests(&h).len(), 1);
    let inbox = body_json(send(&h.state, get("/api/inbox")).await).await;
    let items = inbox.as_array().unwrap();
    assert_eq!(items.len(), 1, "{inbox}");
    assert_eq!(items[0]["kind"], "warn");
    assert_eq!(items[0]["title"], "Could not ask Komga to scan");
    assert!(
        items[0]["body"].as_str().unwrap().contains("401"),
        "{inbox}"
    );
    let task = body_json(send(&h.state, get(&format!("/api/tasks/{id}"))).await).await;
    assert_eq!(task["task"]["status"], "finished");
    assert_eq!(task["chapters"][0]["status"], "downloaded");
}

#[tokio::test(flavor = "multi_thread")]
async fn komga_without_a_library_id_scans_every_library_the_key_can_see() {
    let h = harness(Stub::default(), json!({"library_id": null})).await;
    let id = add(&h.state, "/c/1").await;
    wait_for(&h.state, id, "finished").await;

    after_the_window().await;
    assert_eq!(
        requests(&h),
        [
            seen("GET", "/api/v1/libraries", "key-1"),
            seen("POST", "/api/v1/libraries/L1/scan", "key-1"),
            seen("POST", "/api/v1/libraries/L2/scan", "key-1"),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_komga_library_failing_to_scan_does_not_stop_the_others() {
    let stub = Stub {
        broken: Some("/api/v1/libraries/L1/scan"),
        ..Stub::default()
    };
    let h = harness(stub, json!({"library_id": null})).await;
    let id = add(&h.state, "/c/1").await;
    wait_for(&h.state, id, "finished").await;

    after_the_window().await;
    assert_eq!(
        requests(&h).last(),
        Some(&seen("POST", "/api/v1/libraries/L2/scan", "key-1"))
    );
    let inbox = body_json(send(&h.state, get("/api/inbox")).await).await;
    assert_eq!(inbox[0]["title"], "Could not ask Komga to scan", "{inbox}");
    assert!(
        inbox[0]["body"].as_str().unwrap().contains("500"),
        "{inbox}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn kavita_is_asked_to_scan_every_library() {
    let h = harness(Stub::default(), json!({"kind": "kavita"})).await;
    let id = add(&h.state, "/c/1").await;
    wait_for(&h.state, id, "finished").await;

    after_the_window().await;
    assert_eq!(
        requests(&h),
        [seen("POST", "/api/Library/scan-all", "key-1")]
    );
}

/// A library series downloading to `folder` (under the harness's folder) with a chapter file
/// there unless `empty`; answers its ID.
fn series(h: &Harness, folder: &str, empty: bool) -> i64 {
    let save_to = h.dir.path().join(folder);
    if !empty {
        fs::create_dir_all(&save_to).unwrap();
        fs::write(save_to.join("Chapter 1.cbz"), "chapter 1").unwrap();
    }
    h.db.favorites()
        .create(&NewFavorite {
            module_id: "t".into(),
            link: format!("/{folder}"),
            title: folder.into(),
            save_to: save_to.to_string_lossy().into_owned(),
            cover_url: None,
        })
        .unwrap()
        .id
        .0
}

/// Changes series `id`'s folder to `folder` with `move_files` and waits for the move to end.
async fn move_series(h: &Harness, id: i64, folder: &str) {
    let to = h.dir.path().join(folder);
    let req = Request::patch(format!("/api/favorites/{id}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"save_to": to, "move_files": true}).to_string(),
        ))
        .unwrap();
    assert_eq!(send(&h.state, req).await.status(), StatusCode::OK);
    let wait = async {
        loop {
            let job = body_json(send(&h.state, get("/api/jobs/move-downloads")).await).await;
            if job["state"] == "done" {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    if tokio::time::timeout(PATIENCE, wait).await.is_err() {
        panic!("the move did not end");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_that_moved_files_leads_to_one_scan_after_the_window() {
    let h = harness(Stub::default(), json!({})).await;
    let id = series(&h, "old", false);
    move_series(&h, id, "new").await;
    assert_eq!(requests(&h), [], "not before the window");

    after_the_window().await;
    assert_eq!(
        requests(&h),
        [seen("POST", "/api/v1/libraries/L1/scan", "key-1")]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_that_moved_nothing_leads_to_no_scan() {
    let h = harness(Stub::default(), json!({})).await;
    let id = series(&h, "old", true);
    move_series(&h, id, "new").await;

    after_the_window().await;
    assert_eq!(requests(&h), []);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_without_a_comic_server_set_up_asks_nothing() {
    let h = harness(Stub::default(), json!({"kind": "none"})).await;
    let id = series(&h, "old", false);
    move_series(&h, id, "new").await;

    after_the_window().await;
    assert_eq!(requests(&h), []);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_finishing_and_a_move_ending_within_the_window_give_one_scan() {
    let h = harness(Stub::default(), json!({})).await;
    let id = series(&h, "old", false);
    let task = add(&h.state, "/c/1").await;
    wait_for(&h.state, task, "finished").await;
    move_series(&h, id, "new").await;

    after_the_window().await;
    assert_eq!(
        requests(&h),
        [seen("POST", "/api/v1/libraries/L1/scan", "key-1")]
    );
}

/// `POST /api/settings/comic-server/test` with `body`.
async fn test_connection(h: &Harness, body: Value) -> Value {
    let res = send(
        &h.state,
        post("/api/settings/comic-server/test", Some(body)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await
}

#[tokio::test(flavor = "multi_thread")]
async fn testing_komga_lists_its_libraries_with_the_stored_key() {
    let h = harness(Stub::default(), json!({})).await;
    let url = h.state.settings().get().comic_server.url.clone();

    let result = test_connection(&h, json!({"kind": "komga", "url": url})).await;
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        result["libraries"],
        json!([{"id": "L1", "name": "Manga"}, {"id": "L2", "name": "Comics"}])
    );
    assert_eq!(requests(&h), [seen("GET", "/api/v1/libraries", "key-1")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn testing_uses_a_typed_key_over_the_stored_one() {
    let h = harness(Stub::default(), json!({})).await;
    let url = h.state.settings().get().comic_server.url.clone();

    let result = test_connection(
        &h,
        json!({"kind": "kavita", "url": url, "api_key": "typed"}),
    )
    .await;
    assert_eq!(result["ok"], true, "{result}");
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("2027-01-01T00:00:00Z"),
        "{result}"
    );
    assert_eq!(
        requests(&h),
        [seen("GET", "/api/plugin/authkey-expires", "typed")]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn testing_a_server_answering_401_reports_the_failure() {
    let stub = Stub {
        unauthorized: true,
        ..Stub::default()
    };
    let h = harness(stub, json!({})).await;
    let url = h.state.settings().get().comic_server.url.clone();

    let result = test_connection(&h, json!({"kind": "komga", "url": url})).await;
    assert_eq!(result["ok"], false);
    assert!(
        result["message"].as_str().unwrap().contains("401"),
        "{result}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn testing_another_server_does_not_send_it_the_stored_key() {
    let h = harness(Stub::default(), json!({})).await;
    let url = h.state.settings().get().comic_server.url.clone();

    test_connection(&h, json!({"kind": "kavita", "url": url})).await;
    assert_eq!(
        requests(&h),
        [Seen {
            method: "GET".into(),
            path: "/api/plugin/authkey-expires".into(),
            key: Some(String::new()),
        }]
    );
}
