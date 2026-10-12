//! The MangaBaka database through the HTTP API (T73, issue #204, "Seams
//! under test"), over fmd-core's recorded dump (crates/fmd-core/tests/fixtures/mangabaka).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::info::{InfoError, InfoOptions, MangaInfo};
use fmd_core::metadata::{
    Download, DumpSource, MangaBakaDb, MangaDexLinks, Matcher, MetadataError, MetadataJobs,
};
use fmd_core::modules::ModuleInfo;
use fmd_http::WireResponse;
use fmd_http::{BoxFuture, HttpClient, TerminateToken, Transport, TransportError, WireRequest};
use fmd_server::{AppState, ModuleCatalog, ModulesReport, ServerEvent, build_router};
use fmd_store::{AppDb, ListsDb, MangaListing, MatchConfidence, MatchInput, StoredMatch};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// The recorded dump, compressed as MangaBaka serves it.
struct FixtureDump;

impl DumpSource for FixtureDump {
    fn open(&self, _url: &str, _terminate: &TerminateToken) -> Result<Download, MetadataError> {
        let jsonl = std::fs::read(format!(
            "{}/../fmd-core/tests/fixtures/mangabaka/series.jsonl",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let body = zstd::encode_all(jsonl.as_slice(), 3).unwrap();
        let length = Some(body.len() as u64);
        let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(body));
        Ok(Download { reader, length })
    }
}

/// No MangaDex module is listed here, so nothing asks it.
struct NoNetwork;

impl Transport for NoNetwork {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("no network in tests".into())) })
    }
}

struct Modules;

impl ModuleCatalog for Modules {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        [
            ("fire", "MangaFire", "https://mangafire.to"),
            (PLAIN, "Plain", PLAIN_ROOT),
        ]
        .into_iter()
        .map(|(id, name, root_url)| ModuleInfo {
            id: id.into(),
            name: name.into(),
            root_url: root_url.into(),
            category: "English".into(),
            limits: Default::default(),
            options: Vec::new(),
            capabilities: Default::default(),
        })
        .collect()
    }

    /// Every series of [`PLAIN`] is Reborn Rich.
    fn get_info(
        &self,
        id: &str,
        link: &str,
        _options: InfoOptions,
    ) -> futures_util::future::BoxFuture<'static, Result<MangaInfo, InfoError>> {
        let result = if id == PLAIN {
            Ok(MangaInfo {
                title: "Reborn Rich".into(),
                link: link.into(),
                authors: "San-Gyeong, JP".into(),
                status: "1".into(),
                ..MangaInfo::default()
            })
        } else {
            Err(InfoError::UnknownModule)
        };
        Box::pin(std::future::ready(result))
    }
}

/// A module whose list was never downloaded.
const PLAIN: &str = "plain";
const PLAIN_ROOT: &str = "https://plain.test";

fn listing(link: &str, title: &str, authors: &str) -> MangaListing {
    MangaListing {
        link: link.into(),
        title: title.into(),
        authors: authors.into(),
        ..MangaListing::default()
    }
}

fn state() -> (TempDir, AppState, ListsDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    lists
        .masterlist()
        .replace_module(
            "fire",
            [
                listing("/shadow", "Shadow Star☆", "Mohiro Kitoh"),
                listing(
                    "/baskerville",
                    "Revenge of the Baskerville Bloodhound",
                    "Lego Balbasseo",
                ),
                listing("/onepiece", "One Piece", "Eiichirou Oda"),
                listing("/unknown", "A Title Nobody Has", ""),
            ],
        )
        .unwrap();
    let state = AppState::new(db)
        .unwrap()
        .with_modules(Modules)
        .with_lists(lists.clone());
    (dir, state, lists)
}

fn metadata_jobs(dir: &TempDir, state: &AppState, lists: &ListsDb) -> MetadataJobs {
    let db = Arc::new(MangaBakaDb::open(dir.path(), Arc::new(FixtureDump)));
    metadata_jobs_over(db, dir, state, lists)
}

/// Over a database built from the fixture.
fn built_metadata_jobs(dir: &TempDir, state: &AppState, lists: &ListsDb) -> MetadataJobs {
    let db = Arc::new(MangaBakaDb::open(dir.path(), Arc::new(FixtureDump)));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    metadata_jobs_over(db, dir, state, lists)
}

fn metadata_jobs_over(
    db: Arc<MangaBakaDb>,
    dir: &TempDir,
    state: &AppState,
    lists: &ListsDb,
) -> MetadataJobs {
    let http = HttpClient::with_transport(Arc::new(NoNetwork)).unwrap();
    MetadataJobs::new(
        db,
        Matcher::new(
            lists.clone(),
            AppDb::open(dir.path().join("app.db")).unwrap(),
            MangaDexLinks::new(http),
        ),
        lists.clone(),
        |id: &str| match id {
            "fire" => Some("https://mangafire.to".to_owned()),
            PLAIN => Some(PLAIN_ROOT.to_owned()),
            _ => None,
        },
        state.settings().clone(),
        state.metadata_events(),
    )
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

async fn get(state: &AppState, uri: &str) -> Value {
    let res = send(state, Request::get(uri).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK, "{uri}");
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

/// Stores matches for the `fire` list as matching would: Shadow Star☆ a completed manga,
/// Baskerville an ongoing manhwa, One Piece rejected.
fn store_matches(lists: &ListsDb) {
    let inputs = lists.matches().all("fire", "test").unwrap();
    let decided: Vec<(MatchInput, StoredMatch)> = inputs
        .into_iter()
        .filter_map(|input| {
            let m = match input.link.as_str() {
                "/shadow" => accepted(2092, "manga", Some("completed")),
                "/baskerville" => accepted(808, "manhwa", Some("ongoing")),
                "/onepiece" => StoredMatch {
                    series_id: None,
                    confidence: MatchConfidence::Ambiguous,
                    format: None,
                    publication: None,
                    year: None,
                },
                _ => return None,
            };
            Some((input, m))
        })
        .collect();
    lists
        .matches()
        .store("fire", decided.iter().map(|(i, m)| (i, m)))
        .unwrap();
}

fn accepted(id: i64, format: &str, status: Option<&str>) -> StoredMatch {
    StoredMatch {
        series_id: Some(id),
        confidence: MatchConfidence::TitleAuthor,
        format: Some(format.into()),
        publication: status.map(str::to_owned),
        year: Some(2000),
    }
}

#[tokio::test]
async fn discover_filters_by_format_and_publication_status() {
    let (_dir, state, lists) = state();
    store_matches(&lists);

    let body = get(&state, "/api/lists/search?module=fire&format=manhwa").await;
    assert_eq!(titles(&body), ["Revenge of the Baskerville Bloodhound"]);
    assert_eq!(body["items"][0]["format"], "manhwa");
    assert_eq!(body["items"][0]["publication"], "ongoing");

    let body = get(
        &state,
        "/api/lists/search?module=fire&publication=completed",
    )
    .await;
    assert_eq!(titles(&body), ["Shadow Star☆"]);

    // Titles without an accepted match stay listed, as "unknown".
    let body = get(&state, "/api/lists/search?module=fire&format=unknown").await;
    assert_eq!(titles(&body), ["A Title Nobody Has", "One Piece"]);
    assert_eq!(body["items"][1]["format"], "unknown");
    assert_eq!(body["items"][1]["publication"], "unknown");

    let body = get(&state, "/api/lists/facets?module=fire").await;
    assert_eq!(
        body["formats"],
        json!([
            { "value": "unknown", "count": 2 },
            { "value": "manga", "count": 1 },
            { "value": "manhwa", "count": 1 },
        ])
    );
    assert_eq!(
        body["publications"],
        json!([
            { "value": "unknown", "count": 2 },
            { "value": "completed", "count": 1 },
            { "value": "ongoing", "count": 1 },
        ])
    );
}

#[tokio::test]
async fn without_the_database_discover_works_as_before() {
    let (_dir, state, _lists) = state();

    let body = get(&state, "/api/lists/search?module=fire").await;
    assert_eq!(body["total"], 4);
    let body = get(&state, "/api/metadata/mangabaka").await;
    assert_eq!(body["available"], false);
    assert_eq!(body["downloaded"], false);
}

/// The next `job.metadata.*` event name on `events`.
async fn next_metadata_event(
    events: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
) -> (String, ServerEvent) {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .unwrap()
            .unwrap();
        if event.name().starts_with("job.metadata.") {
            return (event.name().to_owned(), event);
        }
    }
}

#[tokio::test]
async fn the_database_job_reports_its_start_progress_and_finish() {
    let (dir, state, lists) = state();
    let jobs = metadata_jobs(&dir, &state, &lists);
    let state = state.with_metadata(jobs);
    let mut events = state.events().subscribe();
    let before = get(&state, "/api/metadata/mangabaka").await;
    assert_eq!(before["available"], true);
    assert_eq!(before["downloaded"], false);

    let res = send(
        &state,
        Request::post("/api/metadata/mangabaka/download")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);

    let mut names = Vec::new();
    loop {
        let (name, _) = next_metadata_event(&mut events).await;
        let last = name != "job.metadata.started" && name != "job.metadata.progress";
        names.push(name);
        if last {
            break;
        }
    }
    assert_eq!(
        names.first().map(String::as_str),
        Some("job.metadata.started")
    );
    assert!(
        names.iter().any(|n| n == "job.metadata.progress"),
        "{names:?}"
    );
    assert_eq!(
        names.last().map(String::as_str),
        Some("job.metadata.finished")
    );

    let after = get(&state, "/api/metadata/mangabaka").await;
    assert_eq!(after["downloaded"], true);
    assert_eq!(after["running"], false);
    assert!(after["bytes"].as_u64().unwrap() > 0);
    assert!(after["built_at"].as_str().unwrap().starts_with("20"));
    // The lists were matched against it.
    let body = get(&state, "/api/lists/search?module=fire&format=manhwa").await;
    assert_eq!(titles(&body), ["Revenge of the Baskerville Bloodhound"]);

    let res = send(
        &state,
        Request::delete("/api/metadata/mangabaka")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let removed = get(&state, "/api/metadata/mangabaka").await;
    assert_eq!(removed["downloaded"], false);
    let body = get(&state, "/api/lists/search?module=fire&format=unknown").await;
    assert_eq!(body["total"], 4);
}

#[tokio::test]
async fn a_favorite_added_without_its_websites_list_gets_mangabaka_metadata() {
    let (dir, state, lists) = state();
    let jobs = built_metadata_jobs(&dir, &state, &lists);
    let state = state.with_metadata(jobs);

    let res = send(
        &state,
        Request::post("/api/favorites")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "module_id": PLAIN, "link": "/reborn-rich" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // Matched in the background.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let series = loop {
        let series = get(&state, "/api/series?module=plain&link=%2Freborn-rich").await;
        if !series["format"].is_null() {
            break series;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never matched: {series}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    // Series 189 in the fixture.
    assert_eq!(series["format"], "manhwa");
    assert_eq!(series["year"], 2022);
}
