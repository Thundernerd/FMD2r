//! Discover's list endpoints (`/api/lists/search`, `/api/lists/facets`, `/api/modules`) driven
//! through `build_router` with `oneshot` over a fixture `lists.db`
//! (docs/tickets/T26-discover-list-update.md, "Seams under test").
//!
//! Filters behave like FMD2's `Filter` (baseunits/DBDataProcess.pas:1367-1430): an included
//! genre must occur in `genres`, an excluded one must not, and the status matches exactly.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::jobs::JobRegistry;
use fmd_core::lists::{DbImporter, ListJobs, ListUpdater};
use fmd_core::modules::ModuleInfo;
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_server::{AppState, ModuleCatalog, ModulesReport, ServerEvent, build_router};
use fmd_store::{AppDb, ListsDb, MangaListing};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn listing(link: &str, title: &str, genres: &str, status: &str) -> MangaListing {
    MangaListing {
        link: link.into(),
        title: title.into(),
        genres: genres.into(),
        status: status.into(),
        numchapter: 3,
        added_jdn: 2_460_000,
        ..MangaListing::default()
    }
}

struct Modules;

impl ModuleCatalog for Modules {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        ["a", "b", "c"]
            .into_iter()
            .map(|id| ModuleInfo {
                id: id.into(),
                name: id.to_uppercase(),
                root_url: format!("https://{id}.example"),
                category: "English".into(),
                limits: Default::default(),
                options: Vec::new(),
                capabilities: Default::default(),
            })
            .collect()
    }
}

fn state() -> (TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    lists
        .masterlist()
        .replace_module(
            "a",
            [
                listing("/1", "Alpha", "Action, Adventure", "1"),
                listing("/2", "Beta", "Action, Romance", "0"),
                listing("/3", "Gamma", "Comedy", "1"),
                listing("/4", "Delta", "Action", "0"),
            ],
        )
        .unwrap();
    lists
        .masterlist()
        .replace_module("b", [listing("/9", "Alpha Two", "Action", "1")])
        .unwrap();
    let state = AppState::new(db)
        .unwrap()
        .with_modules(Modules)
        .with_lists(lists);
    (dir, state)
}

async fn get(state: &AppState, uri: &str) -> Response {
    let req = Request::get(uri).body(Body::empty()).unwrap();
    build_router(state.clone()).oneshot(req).await.unwrap()
}

async fn json_of(res: Response) -> Value {
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Selects `modules` as Discover's websites (`general.selected_websites`).
async fn select(state: &AppState, modules: &[&str]) {
    let patch = json!({ "general": { "selected_websites": modules } });
    let req = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from(patch.to_string()))
        .unwrap();
    let res = build_router(state.clone()).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

fn titles(body: &Value) -> Vec<&str> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn search_includes_and_excludes_genres() {
    let (_dir, state) = state();

    let body = json_of(
        get(
            &state,
            "/api/lists/search?module=a&genres_include=Action&genres_exclude=Romance",
        )
        .await,
    )
    .await;

    assert_eq!(titles(&body), ["Alpha", "Delta"]);
    assert_eq!(body["total"], 2);
    assert_eq!(
        body["items"][0],
        json!({
            "module_id": "a",
            "link": "/1",
            "title": "Alpha",
            "alttitles": "",
            "authors": "",
            "artists": "",
            "genres": ["Action", "Adventure"],
            "status": "1",
            "numchapter": 3,
            "added_jdn": 2_460_000,
            "format": "unknown",
            "publication": "unknown",
        })
    );
}

#[tokio::test]
async fn search_filters_by_status_and_spans_modules_without_one() {
    let (_dir, state) = state();
    select(&state, &["a", "b"]).await;

    let body = json_of(get(&state, "/api/lists/search?status=0").await).await;
    assert_eq!(titles(&body), ["Beta", "Delta"]);

    let body = json_of(get(&state, "/api/lists/search?q=alp").await).await;
    assert_eq!(titles(&body), ["Alpha", "Alpha Two"]);
    assert_eq!(body["items"][1]["module_id"], "b");
}

#[tokio::test]
async fn search_without_a_module_covers_only_the_selected_websites() {
    let (_dir, state) = state();
    // `gone` is not loaded and is ignored (mangadownloader/forms/frmMain.pas:6464-6470).
    select(&state, &["b", "gone"]).await;

    let body = json_of(get(&state, "/api/lists/search?q=alp").await).await;
    assert_eq!(titles(&body), ["Alpha Two"]);
    assert_eq!(body["total"], 1);
    let body = json_of(get(&state, "/api/lists/facets").await).await;
    assert_eq!(body["genres"], json!([{ "value": "Action", "count": 1 }]));

    // A module filter still reaches every loaded module.
    let body = json_of(get(&state, "/api/lists/search?module=a&q=alp").await).await;
    assert_eq!(titles(&body), ["Alpha"]);
}

#[tokio::test]
async fn search_without_a_module_finds_nothing_when_no_website_is_selected() {
    let (_dir, state) = state();

    let body = json_of(get(&state, "/api/lists/search").await).await;
    assert_eq!(titles(&body), Vec::<&str>::new());
    assert_eq!(body["total"], 0);
    let body = json_of(get(&state, "/api/lists/facets").await).await;
    assert_eq!(
        body,
        json!({ "genres": [], "statuses": [], "formats": [], "publications": [] })
    );
}

#[tokio::test]
async fn search_pages_through_the_results() {
    let (_dir, state) = state();

    let body = json_of(get(&state, "/api/lists/search?module=a&page=2&page_size=3").await).await;

    assert_eq!(titles(&body), ["Gamma"]);
    assert_eq!(body["total"], 4);
    assert_eq!(body["page"], 2);
    assert_eq!(body["page_size"], 3);
}

#[tokio::test]
async fn facets_count_genres_and_statuses_of_the_matching_titles() {
    let (_dir, state) = state();

    let body = json_of(get(&state, "/api/lists/facets?module=a").await).await;

    assert_eq!(
        body,
        json!({
            "genres": [
                { "value": "Action", "count": 3 },
                { "value": "Adventure", "count": 1 },
                { "value": "Comedy", "count": 1 },
                { "value": "Romance", "count": 1 },
            ],
            "statuses": [
                { "value": "0", "count": 2 },
                { "value": "1", "count": 2 },
            ],
            "formats": [{ "value": "unknown", "count": 4 }],
            "publications": [{ "value": "unknown", "count": 4 }],
        })
    );

    let body = json_of(get(&state, "/api/lists/facets?module=a&q=gamma").await).await;
    assert_eq!(body["genres"], json!([{ "value": "Comedy", "count": 1 }]));
}

#[tokio::test]
async fn modules_report_their_list_size_and_last_update() {
    let (_dir, state) = state();

    let body = json_of(get(&state, "/api/modules").await).await;

    let modules = body.as_array().unwrap();
    assert_eq!(modules[0]["id"], "a");
    assert_eq!(modules[0]["list_size"], 4);
    assert!(modules[0]["list_updated"].is_string());
    assert_eq!(modules[2]["id"], "c");
    assert_eq!(modules[2]["list_size"], 0);
    assert_eq!(modules[2]["list_updated"], Value::Null);
}

#[tokio::test]
async fn list_jobs_are_unavailable_without_a_runner() {
    let (_dir, state) = state();
    let req = Request::post("/api/lists/a/update")
        .body(Body::empty())
        .unwrap();

    let res = build_router(state).oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// Answers every request with a 404, as FMD2-DB does for a module it has no dump of.
struct NoDumps;

impl Transport for NoDumps {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async {
            Ok(WireResponse {
                status: 404,
                reason: String::new(),
                headers: Vec::new(),
                body: b"404: Not Found".to_vec(),
            })
        })
    }
}

/// `state()` with list jobs for one module, `site` ("Site", which can update its list), whose
/// HTTP goes to `NoDumps`.
fn state_with_list_jobs() -> (TempDir, AppState) {
    let (dir, state) = state();
    std::fs::create_dir_all(dir.path().join("lua/modules")).unwrap();
    std::fs::write(
        dir.path().join("lua/modules/Site.lua"),
        "function Init()\n  local m = NewWebsiteModule()\n  m.ID = 'site'; m.Name = 'Site'; \
         m.RootURL = 'https://site.test'; m.OnGetNameAndLink = 'GetNameAndLink'\nend\n",
    )
    .unwrap();
    let report = ModuleRegistry::load_dir(&dir.path().join("lua"));
    let module = report.registry.get("site").unwrap().clone();
    let http = HttpClient::with_transport(Arc::new(NoDumps)).unwrap();
    let mut config = PoolConfig::new(http.clone());
    config.threads = 1;
    let pool = Arc::new(WorkerPool::new(config).unwrap());
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let events = state.events().clone();
    let jobs = ListJobs::new(
        ListUpdater::new(pool, lists.clone()),
        DbImporter::new(http, lists),
        Arc::new(SettingsService::load(db).unwrap()),
        move |id: &str| (id == "site").then(|| module.clone()),
        move |event| events.publish(ServerEvent::Lists(event)),
    );
    let registry = JobRegistry::new();
    jobs.register(&registry);
    (dir, state.with_jobs(registry).with_list_jobs(jobs))
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
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), read).await;
    assert!(
        timeout.is_ok(),
        "timed out waiting for {needle:?}; got {seen:?}"
    );
    seen
}

#[tokio::test]
async fn a_failed_import_event_carries_why_it_failed() {
    let (_dir, state) = state_with_list_jobs();
    let stream = get(&state, "/api/events").await;
    let req = Request::post("/api/lists/site/import-db")
        .body(Body::empty())
        .unwrap();
    let res = build_router(state.clone()).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);

    let seen = read_sse_until(stream, "event: job.lists.failed\n").await;
    let frame = seen
        .split("\n\n")
        .find(|f| f.contains("job.lists.failed"))
        .unwrap();
    let data = frame
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .unwrap();
    let data: Value = serde_json::from_str(data).unwrap();

    assert_eq!(data["module_id"], "site");
    assert_eq!(data["job"], "import_db");
    assert_eq!(data["reason"], "no_dump");
    // The technical details stay available.
    assert!(data["error"].as_str().unwrap().contains("404"), "{data}");

    // System → Jobs reads the same, naming the website, with the details after it.
    let job = json_of(get(&state, "/api/jobs/lists").await).await;
    let last_error = job["last_error"].as_str().unwrap();
    assert!(
        last_error.starts_with(
            "FMD2-DB has no ready-made list for Site. Use Update list to build it from the website."
        ),
        "{last_error}"
    );
    assert!(last_error.contains("HTTP status 404"), "{last_error}");
}
