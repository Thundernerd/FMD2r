//! Discover's list endpoints (`/api/lists/search`, `/api/lists/facets`, `/api/modules`) driven
//! through `build_router` with `oneshot` over a fixture `lists.db`
//! (docs/tickets/T26-discover-list-update.md, "Seams under test").
//!
//! Filters behave like FMD2's `Filter` (baseunits/DBDataProcess.pas:1367-1430): an included
//! genre must occur in `genres`, an excluded one must not, and the status matches exactly.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::modules::ModuleInfo;
use fmd_server::{AppState, ModuleCatalog, ModulesReport, build_router};
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
        })
    );
}

#[tokio::test]
async fn search_filters_by_status_and_spans_modules_without_one() {
    let (_dir, state) = state();

    let body = json_of(get(&state, "/api/lists/search?status=0").await).await;
    assert_eq!(titles(&body), ["Beta", "Delta"]);

    let body = json_of(get(&state, "/api/lists/search?q=alp").await).await;
    assert_eq!(titles(&body), ["Alpha", "Alpha Two"]);
    assert_eq!(body["items"][1]["module_id"], "b");
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
