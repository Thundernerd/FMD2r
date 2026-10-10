//! Queue endpoints over the real download engine: a `DownloadManager` with a `WorkerPool`
//! running a fixture module, a stub HTTP transport serving images, a temp `app.db` and a temp
//! output dir (docs/tickets/T23-queue.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::download::{DownloadManager, EngineConfig};
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_server::{AppState, build_router};
use fmd_store::AppDb;
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

/// Serves a PNG for every URL holding `/img/`, a 404 otherwise; never answers the URLs holding
/// a held pattern.
#[derive(Default)]
struct StubTransport {
    hold: Mutex<Vec<String>>,
}

impl Transport for StubTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let held = self
            .hold
            .lock()
            .unwrap()
            .iter()
            .any(|p| request.url.contains(p));
        if held {
            return Box::pin(std::future::pending());
        }
        let image = request.url.contains("/img/");
        let response = WireResponse {
            status: if image { 200 } else { 404 },
            reason: String::new(),
            headers: Vec::new(),
            body: if image { PNG.to_vec() } else { Vec::new() },
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

struct Harness {
    dir: TempDir,
    state: AppState,
    transport: Arc<StubTransport>,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let lua = dir.path().join("lua");
    fs::create_dir_all(lua.join("modules")).unwrap();
    fs::write(lua.join("modules/T.lua"), T).unwrap();
    let report = ModuleRegistry::load_dir(&lua);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let transport = Arc::new(StubTransport::default());
    let http = HttpClient::with_transport(transport.clone()).unwrap();
    let mut config = PoolConfig::new(http.clone());
    config.threads = 2;
    config.lua_dir = lua;
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let settings = Arc::new(SettingsService::load(db.clone()).unwrap());
    settings
        .update(json!({
            "output": {"format": "cbz"},
            "saveto": {"default_dir": dir.path().join("out").to_string_lossy()},
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
    let state = AppState::new(db)
        .unwrap()
        .with_settings(settings)
        .with_engine(engine);
    Harness {
        dir,
        state,
        transport,
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

/// Queues chapters `links` of the manga "Manga" and answers the new task's ID.
async fn add(state: &AppState, links: &[&str]) -> i64 {
    let chapters: Vec<Value> = links
        .iter()
        .map(|link| json!({"name": format!("Ch {}", &link[3..]), "link": link}))
        .collect();
    let body = json!({"module_id": "t", "link": "/manga", "title": "Manga", "chapters": chapters});
    let res = send(state, post("/api/tasks", Some(body))).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    body_json(res).await["id"].as_i64().unwrap()
}

async fn wait_for(state: &AppState, id: i64, status: &str) -> Value {
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
    last
}

#[tokio::test(flavor = "multi_thread")]
async fn an_added_task_downloads_and_its_files_stream_as_cbz() {
    let h = harness().await;
    let id = add(&h.state, &["/c/2"]).await;

    let res = send(&h.state, get(&format!("/api/tasks/{id}"))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let detail = body_json(res).await;
    assert_eq!(detail["task"]["title"], "Manga");
    assert_eq!(detail["task"]["module_id"], "t");
    // FMD2 pads chapter numbers to 3 digits by default (`OptionConvertDigitChapterLength`).
    assert_eq!(detail["chapters"][0]["name"], "Ch 002");

    let detail = wait_for(&h.state, id, "finished").await;
    assert_eq!(detail["chapters"][0]["status"], "downloaded");
    let res = send(&h.state, get(&format!("/api/tasks/{id}/files"))).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()["content-type"],
        "application/vnd.comicbook+zip"
    );
    assert_eq!(
        res.headers()["content-disposition"],
        "attachment; filename=\"Ch 002.cbz\"; filename*=UTF-8''Ch%20002.cbz"
    );
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let pages: Vec<&str> = zip.file_names().collect();
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(h.dir.path().join("out/Manga/Ch 002.cbz").is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_downloading_task_leaves_it_stopped() {
    let h = harness().await;
    h.transport.hold.lock().unwrap().push("/img/".into());
    let id = add(&h.state, &["/c/3"]).await;
    wait_for(&h.state, id, "downloading").await;

    let res = send(&h.state, post(&format!("/api/tasks/{id}/stop"), None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    wait_for(&h.state, id, "stopped").await;
    let res = send(&h.state, get("/api/tasks?status=stopped")).await;
    assert_eq!(body_json(res).await["items"][0]["id"], id);
}

/// A task queued with a destination's folder saves there, under the manga folder, and the
/// queue shows that folder (T74).
#[tokio::test(flavor = "multi_thread")]
async fn a_task_queued_to_a_destination_saves_there() {
    let h = harness().await;
    let manhwa = h.dir.path().join("manhwa");
    let manhwa = manhwa.to_string_lossy();
    let patch = json!({"saveto": {"destinations": [
        {"name": "Downloads", "path": h.dir.path().join("out").to_string_lossy(), "default": true},
        {"name": "Manhwa", "path": manhwa},
    ]}});
    let req = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from(patch.to_string()))
        .unwrap();
    assert_eq!(send(&h.state, req).await.status(), StatusCode::OK);
    let body = json!({
        "module_id": "t", "link": "/manga", "title": "Manga",
        "chapters": [{"name": "Ch 2", "link": "/c/2"}],
        "save_to": manhwa,
    });
    let res = send(&h.state, post("/api/tasks", Some(body))).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let id = body_json(res).await["id"].as_i64().unwrap();

    let detail = wait_for(&h.state, id, "finished").await;
    let folder = h.dir.path().join("manhwa").join("Manga");
    assert_eq!(detail["task"]["save_to"], folder.to_string_lossy().as_ref());
    assert!(folder.join("Ch 002.cbz").is_file());
    assert!(!h.dir.path().join("out").exists());
}
