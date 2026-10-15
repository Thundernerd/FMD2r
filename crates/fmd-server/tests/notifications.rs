//! Notifications over the real download engine and favorites check, sent to a local HTTP stub
//! (issue #268, "Seams under test").

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, Uri};
use axum::response::Response;
use fmd_core::download::{DownloadManager, EngineConfig};
use fmd_core::favorites::{CheckMode, CheckScope, CheckerConfig, FavoritesChecker};
use fmd_core::jobs::JobRegistry;
use fmd_core::modules::ModuleInfo;
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_server::{AppState, EventBus, LogBuffer, ModuleCatalog, ModulesReport, build_router};
use fmd_store::{AppDb, NewFavorite};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// How long a test waits for the engine or a notification before failing.
const PATIENCE: Duration = Duration::from_secs(20);

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// The website: a PNG for every URL holding `/img/`, a 404 otherwise.
struct Site;

impl Transport for Site {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
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

/// A module whose series `/manga` has chapters `/c/1` and `/c/2`, and whose chapter `/c/<n>` has
/// `n` pages at `https://t/img/c/<n>/<page>`; chapter `/c/0` (or any link ending in `/0`, such
/// as `/d/0`) has no pages, so it fails.
const T: &str = r#"
function Init()
  local m = NewWebsiteModule(); m.ID='t'; m.Name='T'; m.RootURL='https://t'
  m.OnGetInfo='GI'; m.OnGetPageNumber='GPN'
end
function GI()
  MANGAINFO.Title = 'Manga'
  MANGAINFO.ChapterLinks.Add('/c/1'); MANGAINFO.ChapterNames.Add('Ch 1')
  MANGAINFO.ChapterLinks.Add('/c/2'); MANGAINFO.ChapterNames.Add('Ch 2')
  return no_error
end
function GPN()
  local n = tonumber(URL:match('%d+$'))
  if n == 0 then return false end
  for i = 1, n do TASK.PageLinks.Add('https://t/img' .. URL .. '/' .. i) end
  return true
end
"#;

/// The catalog entry of module `t`, named "Test Site".
struct Modules;

impl ModuleCatalog for Modules {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        vec![ModuleInfo {
            id: "t".into(),
            name: "Test Site".into(),
            root_url: "https://t".into(),
            category: String::new(),
            limits: Default::default(),
            options: Vec::new(),
            capabilities: Default::default(),
        }]
    }
}

/// How the stub answers a path.
#[derive(Clone, Copy)]
enum Answer {
    Ok,
    ServerError,
    /// Never answers.
    Hang,
}

/// One request the stub got.
#[derive(Debug, Clone)]
struct Received {
    path: String,
    headers: HeaderMap,
    body: String,
}

#[derive(Default)]
struct StubState {
    received: Mutex<Vec<Received>>,
    answers: Mutex<Vec<(String, Answer)>>,
}

/// A local HTTP server standing in for ntfy, Discord and the webhook.
struct Stub {
    url: String,
    state: Arc<StubState>,
}

impl Stub {
    async fn start() -> Stub {
        let state = Arc::new(StubState::default());
        let app = Router::new().fallback(record).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Stub { url, state }
    }

    fn answer(&self, path: &str, answer: Answer) {
        self.state
            .answers
            .lock()
            .unwrap()
            .push((path.into(), answer));
    }

    fn received(&self) -> Vec<Received> {
        self.state.received.lock().unwrap().clone()
    }

    /// The paths requested, sorted, once `count` requests came in and no more followed shortly.
    async fn paths_after(&self, count: usize) -> Vec<String> {
        let start = Instant::now();
        while self.received().len() < count {
            assert!(
                start.elapsed() < PATIENCE,
                "got {:?}, wanted {count} requests",
                self.received()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Long enough for a stray extra request to arrive.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut paths: Vec<String> = self.received().into_iter().map(|r| r.path).collect();
        paths.sort();
        paths
    }
}

async fn record(
    State(state): State<Arc<StubState>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let path = uri.path().to_owned();
    state.received.lock().unwrap().push(Received {
        path: path.clone(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    });
    let answer = state
        .answers
        .lock()
        .unwrap()
        .iter()
        .find(|(p, _)| *p == path)
        .map_or(Answer::Ok, |(_, a)| *a);
    match answer {
        Answer::Ok => StatusCode::OK,
        Answer::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
        Answer::Hang => std::future::pending().await,
    }
}

struct Harness {
    _dir: TempDir,
    state: AppState,
    checker: FavoritesChecker,
    stub: Stub,
}

/// ntfy (both kinds of event) at `/ntfy`, Discord (new chapters only) at `/discord`, and a
/// disabled webhook at `/webhook`, all on the stub.
async fn harness(logs: LogBuffer) -> Harness {
    let stub = Stub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let lua = dir.path().join("lua");
    fs::create_dir_all(lua.join("modules")).unwrap();
    fs::write(lua.join("modules/T.lua"), T).unwrap();
    let report = ModuleRegistry::load_dir(&lua);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let registry = Arc::new(report.registry);
    let http = HttpClient::with_transport(Arc::new(Site)).unwrap();
    let mut config = PoolConfig::new(http.clone());
    config.threads = 2;
    config.lua_dir = lua;
    let pool = Arc::new(WorkerPool::new(config).unwrap());
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let settings = Arc::new(SettingsService::load(db.clone()).unwrap());
    let out = dir.path().join("out");
    settings
        .update(json!({
            "connections": {"auto_retry_failed_tasks": 0},
            "saveto": {"default_dir": out.to_string_lossy()},
            "notifications": {
                "ntfy": {"enabled": true, "topic_url": format!("{}/ntfy", stub.url),
                         "token": "tk"},
                "discord": {"enabled": true, "webhook_url": format!("{}/discord", stub.url),
                            "types": ["new_chapters", "missing_chapters"]},
                "webhook": {"enabled": false, "url": format!("{}/webhook", stub.url)},
            },
        }))
        .unwrap();
    let engine = DownloadManager::open(EngineConfig::new(
        db.clone(),
        pool.clone(),
        registry.clone(),
        settings.clone(),
        http,
    ))
    .await
    .unwrap();
    let state = AppState::new(db.clone())
        .unwrap()
        .with_logs(logs)
        .with_settings(settings.clone())
        .with_engine(engine)
        .with_modules(Modules);
    let checker = FavoritesChecker::new(
        CheckerConfig {
            db: db.clone(),
            pool,
            modules: Arc::new(move |id| registry.get(id).cloned()),
            settings,
            queue: None,
            jobs: JobRegistry::new(),
            rate_limits: Arc::default(),
        },
        state.favorites_events(),
    );
    let state = state.with_favorites(checker.clone());
    state.start_notifications();
    db.downloaded_chapters()
        .mark("t", "/manga", &["/c/1"])
        .unwrap();
    db.favorites()
        .create(&NewFavorite {
            module_id: "t".into(),
            link: "/manga".into(),
            title: "Manga".into(),
            save_to: out.to_string_lossy().into_owned(),
            cover_url: None,
        })
        .unwrap();
    Harness {
        _dir: dir,
        state,
        checker,
        stub,
    }
}

fn logs() -> LogBuffer {
    LogBuffer::new(100, EventBus::new())
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn body_json(res: Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Queues chapter `link` of "Manga" and answers the new task's ID.
async fn add(state: &AppState, link: &str) -> i64 {
    add_chapters(state, &[("Ch", link)]).await
}

/// Queues `chapters` (name, link) of "Manga" as one task and answers its ID.
async fn add_chapters(state: &AppState, chapters: &[(&str, &str)]) -> i64 {
    let chapters: Vec<Value> = chapters
        .iter()
        .map(|(name, link)| json!({"name": name, "link": link}))
        .collect();
    let body = json!({"module_id": "t", "link": "/manga", "title": "Manga",
        "chapters": chapters});
    let req = Request::post("/api/tasks")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = send(state, req).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    body_json(res).await["id"].as_i64().unwrap()
}

async fn wait_for(state: &AppState, id: i64, status: &str) {
    let start = Instant::now();
    loop {
        let res = send(state, get(&format!("/api/tasks/{id}"))).await;
        let last = body_json(res).await;
        if last["task"]["status"] == status {
            return;
        }
        assert!(
            start.elapsed() < PATIENCE,
            "task {id} did not reach {status}; last {last}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn header<'a>(received: &'a Received, name: &str) -> Option<&'a str> {
    received.headers.get(name).and_then(|v| v.to_str().ok())
}

#[tokio::test(flavor = "multi_thread")]
async fn new_chapters_go_to_every_enabled_channel_that_ticks_them() {
    let h = harness(logs()).await;

    let report = h
        .checker
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();
    assert_eq!(report.found.len(), 1);

    assert_eq!(h.stub.paths_after(2).await, ["/discord", "/ntfy"]);
    let received = h.stub.received();
    let ntfy = received.iter().find(|r| r.path == "/ntfy").unwrap();
    assert_eq!(header(ntfy, "title"), Some("Found new chapter(s)"));
    assert_eq!(header(ntfy, "authorization"), Some("Bearer tk"));
    assert!(ntfy.body.contains("Manga"), "{}", ntfy.body);
    let discord = received.iter().find(|r| r.path == "/discord").unwrap();
    let content: Value = serde_json::from_str(&discord.body).unwrap();
    assert!(
        content["content"]
            .as_str()
            .unwrap()
            .starts_with("**Found new chapter(s)**\n"),
        "{content}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_download_shows_in_the_inbox_and_goes_to_the_channels_that_tick_it() {
    let h = harness(logs()).await;

    let id = add(&h.state, "/c/0").await;
    wait_for(&h.state, id, "failed").await;

    assert_eq!(h.stub.paths_after(1).await, ["/ntfy"]);
    let ntfy = &h.stub.received()[0];
    assert_eq!(header(ntfy, "priority"), Some("4"));
    assert_eq!(header(ntfy, "title"), Some("Download failed: Manga"));
    let inbox = body_json(send(&h.state, get("/api/inbox")).await).await;
    let item = &inbox[0];
    assert_eq!(item["kind"], "error");
    assert_eq!(item["title"], "Download failed: Manga");
}

/// "Download completed" is off by default; ticked on the webhook only, a finished task goes there
/// and nowhere else, and not to the inbox (issue #300).
#[tokio::test(flavor = "multi_thread")]
async fn a_finished_download_goes_only_to_channels_that_tick_it_and_not_to_the_inbox() {
    let h = harness(logs()).await;
    let patch = json!({"notifications": {"webhook": {"enabled": true,
        "types": ["download_completed"]}}});
    let req = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from(patch.to_string()))
        .unwrap();
    assert_eq!(send(&h.state, req).await.status(), StatusCode::OK);

    let id = add(&h.state, "/c/2").await;
    wait_for(&h.state, id, "finished").await;

    assert_eq!(h.stub.paths_after(1).await, ["/webhook"]);
    let body: Value = serde_json::from_str(&h.stub.received()[0].body).unwrap();
    assert_eq!(body["kind"], "download_completed");
    assert_eq!(body["severity"], "info");
    assert_eq!(body["title"], "Download completed: Manga");
    assert_eq!(body["body"], "Chapters: Ch\nWebsite: Test Site");
    let inbox = body_json(send(&h.state, get("/api/inbox")).await).await;
    assert_eq!(inbox, json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_downloads_inbox_item_and_notification_say_why_it_failed() {
    let h = harness(logs()).await;

    let id = add(&h.state, "/c/0").await;
    wait_for(&h.state, id, "failed").await;

    // The engine's reason for the chapter, not the "No reason was given." fallback.
    let reason = "Chapter \"Ch\" has no pages";
    let inbox = body_json(send(&h.state, get("/api/inbox")).await).await;
    assert_eq!(inbox[0]["body"], reason, "{inbox}");
    assert_eq!(h.stub.paths_after(1).await, ["/ntfy"]);
    let ntfy = &h.stub.received()[0];
    assert!(ntfy.body.contains(reason), "{}", ntfy.body);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_failing_two_chapters_makes_one_inbox_item_naming_both() {
    let h = harness(logs()).await;

    let id = add_chapters(
        &h.state,
        &[("Ch A", "/c/0"), ("Ch B", "/c/2"), ("Ch C", "/d/0")],
    )
    .await;
    wait_for(&h.state, id, "failed").await;

    assert_eq!(h.stub.paths_after(1).await, ["/ntfy"]);
    let inbox = body_json(send(&h.state, get("/api/inbox")).await).await;
    let items = inbox.as_array().unwrap();
    assert_eq!(items.len(), 1, "{inbox}");
    assert_eq!(items[0]["title"], "Download failed: Manga");
    assert_eq!(
        items[0]["body"],
        "Chapters failed: Chapter \"Ch A\" has no pages; Chapter \"Ch C\" has no pages"
    );
}

/// A channel answering 500 and one that never answers delay neither the check nor a download,
/// and each failure is logged as a warning naming the channel.
#[tokio::test]
async fn a_broken_channel_neither_delays_nor_fails_the_work_and_is_logged() {
    use tracing_subscriber::layer::SubscriberExt;
    let logs = logs();
    // A current-thread runtime runs every task on this thread, under this subscriber.
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(logs.clone()));
    let h = harness(logs).await;
    h.stub.answer("/ntfy", Answer::ServerError);
    h.stub.answer("/discord", Answer::Hang);

    let start = Instant::now();
    let report = h
        .checker
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();
    assert_eq!(report.found.len(), 1);
    let id = add(&h.state, "/c/2").await;
    wait_for(&h.state, id, "finished").await;
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "took {:?}",
        start.elapsed()
    );

    // The hanging channel times out after a few seconds.
    let start = Instant::now();
    let warnings = loop {
        let lines = body_json(send(&h.state, get("/api/logs?level=warn")).await).await;
        let messages: Vec<String> = lines
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["message"].as_str().unwrap().to_owned())
            .collect();
        if messages.len() >= 2 || start.elapsed() > PATIENCE {
            break messages;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(
        warnings
            .iter()
            .any(|m| m.contains("ntfy") && m.contains("500")),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|m| m.contains("Discord") && m.contains("timed out")),
        "{warnings:?}"
    );
}

async fn send_test(state: &AppState, channel: &str) -> Value {
    let req = Request::post(format!("/api/notifications/{channel}/test"))
        .body(Body::empty())
        .unwrap();
    let res = send(state, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await
}

/// "Send test" uses the saved settings, even of a channel that is off, and says how it went.
#[tokio::test(flavor = "multi_thread")]
async fn a_test_notification_reports_whether_it_was_delivered() {
    let h = harness(logs()).await;
    h.stub.answer("/ntfy", Answer::ServerError);

    assert_eq!(
        send_test(&h.state, "webhook").await,
        json!({"delivered": true, "error": null})
    );
    let received = h.stub.received();
    assert_eq!(received[0].path, "/webhook");
    let body: Value = serde_json::from_str(&received[0].body).unwrap();
    assert_eq!(body["kind"], "test");

    assert_eq!(
        send_test(&h.state, "ntfy").await,
        json!({"delivered": false, "error": "HTTP 500"})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_test_notification_to_a_channel_without_a_url_is_not_delivered() {
    let h = harness(logs()).await;
    let patch = json!({"notifications": {"webhook": {"url": ""}}});
    let req = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from(patch.to_string()))
        .unwrap();
    assert_eq!(send(&h.state, req).await.status(), StatusCode::OK);

    let result = send_test(&h.state, "webhook").await;
    assert_eq!(result["delivered"], false);
    assert_eq!(result["error"], "No URL is set.");
    assert!(h.stub.received().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_test_notification_to_an_unknown_channel_is_a_bad_request() {
    let h = harness(logs()).await;
    let req = Request::post("/api/notifications/telegram/test")
        .body(Body::empty())
        .unwrap();
    let res = send(&h.state, req).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
}
