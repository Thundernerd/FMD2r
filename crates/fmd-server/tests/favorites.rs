//! The favorites endpoints driven through `build_router` with `oneshot`, over a fake module
//! catalog and a fake favorites job (docs/tickets/T25-library-favorites.md, "Seams under test").
//!
//! Expected behaviour comes from FMD2's `TMainForm.btAddToFavoritesClick`
//! (mangadownloader/forms/frmMain.pas:2790-2846) and `TFavoriteManager`
//! (baseunits/uFavoritesManager.pas:845-928, :1213-1300).
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::favorites::{CheckError, CheckMode, CheckScope};
use fmd_core::info::{Chapter, InfoError, InfoOptions, MangaInfo};
use fmd_core::modules::ModuleInfo;
use fmd_server::{AppState, FavoritesJobs, ModuleCatalog, ModulesReport, build_router};
use fmd_store::{AppDb, FavoriteId};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// Module `t` ("T"), whose every series is "Manga" with chapters `/c1` and `/c2`.
struct Modules;

impl ModuleCatalog for Modules {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        vec![ModuleInfo {
            id: "t".into(),
            name: "T".into(),
            root_url: "https://example.com".into(),
            category: String::new(),
            limits: Default::default(),
            options: Vec::new(),
            capabilities: Default::default(),
        }]
    }

    fn get_info(
        &self,
        id: &str,
        link: &str,
        _options: InfoOptions,
    ) -> BoxFuture<'static, Result<MangaInfo, InfoError>> {
        let result = if id == "t" {
            Ok(MangaInfo {
                title: "Manga".into(),
                link: link.into(),
                cover_link: "https://example.com/cover.jpg".into(),
                authors: "Author".into(),
                status: "1".into(),
                chapters: vec![
                    Chapter {
                        name: "Chapter 1".into(),
                        link: "/c1".into(),
                    },
                    Chapter {
                        name: "Chapter 2".into(),
                        link: "/c2".into(),
                    },
                ],
                ..MangaInfo::default()
            })
        } else {
            Err(InfoError::UnknownModule)
        };
        Box::pin(std::future::ready(result))
    }
}

/// Records the checks it is asked to start.
#[derive(Clone, Default)]
struct FakeJobs {
    calls: Arc<Mutex<Vec<(CheckScope, CheckMode)>>>,
}

impl FavoritesJobs for FakeJobs {
    fn check(&self, scope: CheckScope, mode: CheckMode) -> Result<(), CheckError> {
        self.calls.lock().unwrap().push((scope, mode));
        Ok(())
    }
}

struct Fixture {
    _dir: TempDir,
    db: AppDb,
    jobs: FakeJobs,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        Fixture {
            _dir: dir,
            db,
            jobs: FakeJobs::default(),
        }
    }

    fn state(&self) -> AppState {
        AppState::new(self.db.clone())
            .unwrap()
            .with_modules(Modules)
            .with_favorites(self.jobs.clone())
    }

    async fn send(&self, method: &str, uri: &str, body: Option<Value>) -> Response {
        let builder = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        };
        build_router(self.state())
            .oneshot(request.unwrap())
            .await
            .unwrap()
    }

    async fn add(&self) -> Value {
        let response = self
            .send(
                "POST",
                "/api/favorites",
                Some(json!({ "module_id": "t", "link": "/manga" })),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        json_body(response).await
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn an_added_favorite_is_listed() {
    let fx = Fixture::new();

    let added = fx.add().await;

    let response = fx.send("GET", "/api/favorites", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let list = json_body(response).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let favorite = &list[0];
    assert_eq!(favorite["id"], added["id"]);
    assert_eq!(favorite["module_id"], "t");
    assert_eq!(favorite["website"], "T");
    assert_eq!(favorite["link"], "/manga");
    assert_eq!(favorite["title"], "Manga");
    assert_eq!(favorite["status"], "ongoing");
    assert_eq!(favorite["enabled"], true);
    assert_eq!(favorite["current_chapter"], 2);
    assert_eq!(favorite["new_chapters"], 0);
    assert!(
        favorite["cover_url"]
            .as_str()
            .unwrap()
            .starts_with("/api/covers")
    );
}

#[tokio::test]
async fn adding_a_favorite_takes_its_current_chapters_as_seen() {
    let fx = Fixture::new();

    fx.add().await;

    // `FavoriteManager.Add(..., mangaInfo.ChapterLinks.Text, ...)`: the chapters listed when it
    // was added are not new (mangadownloader/forms/frmMain.pas:2829-2836).
    let downloaded = fx.db.downloaded_chapters().list_for("t", "/manga").unwrap();
    assert_eq!(downloaded, ["/c1", "/c2"]);
}

#[tokio::test]
async fn a_series_is_added_only_once() {
    let fx = Fixture::new();
    fx.add().await;

    let again = fx
        .send(
            "POST",
            "/api/favorites",
            Some(json!({ "module_id": "t", "link": "/manga" })),
        )
        .await;

    assert_eq!(again.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_favorite_can_be_disabled_renamed_and_deleted() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();

    let patched = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{id}"),
            Some(json!({ "enabled": false, "title": "Renamed", "save_to": "/data/Manga" })),
        )
        .await;
    assert_eq!(patched.status(), StatusCode::OK);
    let stored = fx.db.favorites().get(FavoriteId(id)).unwrap().unwrap();
    assert!(!stored.enabled);
    assert_eq!(stored.title, "Renamed");
    assert_eq!(stored.save_to, "/data/Manga");

    let deleted = fx
        .send("DELETE", &format!("/api/favorites/{id}"), None)
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(fx.db.favorites().list().unwrap().is_empty());
    let gone = fx
        .send("DELETE", &format!("/api/favorites/{id}"), None)
        .await;
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_check_endpoint_starts_the_job() {
    let fx = Fixture::new();

    let all = fx.send("POST", "/api/favorites/check", None).await;
    let some = fx
        .send(
            "POST",
            "/api/favorites/check",
            Some(json!({ "ids": [4, 7] })),
        )
        .await;
    let missing = fx
        .send("POST", "/api/favorites/4/check-missing", None)
        .await;

    assert_eq!(all.status(), StatusCode::ACCEPTED);
    assert_eq!(some.status(), StatusCode::ACCEPTED);
    assert_eq!(missing.status(), StatusCode::ACCEPTED);
    assert_eq!(
        *fx.jobs.calls.lock().unwrap(),
        [
            (CheckScope::All, CheckMode::New),
            (
                CheckScope::Only(vec![FavoriteId(4), FavoriteId(7)]),
                CheckMode::New
            ),
            (CheckScope::Only(vec![FavoriteId(4)]), CheckMode::Missing),
        ]
    );
}

#[tokio::test]
async fn filters_pick_favorites_by_state_and_title() {
    let fx = Fixture::new();
    let first = fx.add().await["id"].as_i64().unwrap();
    let second = fx
        .send(
            "POST",
            "/api/favorites",
            Some(json!({ "module_id": "t", "link": "/other" })),
        )
        .await;
    let second = json_body(second).await["id"].as_i64().unwrap();
    fx.send(
        "PATCH",
        &format!("/api/favorites/{second}"),
        Some(json!({ "enabled": false, "title": "Other" })),
    )
    .await;

    let ids = |list: Value| -> Vec<i64> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_i64().unwrap())
            .collect()
    };
    let disabled = json_body(fx.send("GET", "/api/favorites?filter=disabled", None).await).await;
    assert_eq!(ids(disabled), [second]);
    let searched = json_body(fx.send("GET", "/api/favorites?q=mang", None).await).await;
    assert_eq!(ids(searched), [first]);
    let ongoing = json_body(fx.send("GET", "/api/favorites?filter=ongoing", None).await).await;
    assert_eq!(ids(ongoing), [first, second]);
    let completed = json_body(
        fx.send("GET", "/api/favorites?filter=completed", None)
            .await,
    )
    .await;
    assert_eq!(ids(completed), Vec::<i64>::new());
}
