//! The favorites endpoints driven through `build_router` with `oneshot`, over a fake module
//! catalog and a fake favorites job (T25, issue #156, "Seams under test").
//!
//! Expected behaviour comes from FMD2's `TMainForm.btAddToFavoritesClick`
//! (mangadownloader/forms/frmMain.pas:2797-2846) and `TFavoriteManager`
//! (baseunits/uFavoritesManager.pas:832-928, :1213-1300).
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
use fmd_store::{AppDb, FavoriteId, ImportedFavorite, NewFavorite};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// Module `t` ("T"), whose every series is "Manga" in genres " Action, drama, Action," with
/// chapters `/c1` and `/c2`.
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
                genres: " Action, drama, Action,".into(),
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
async fn a_library_series_is_tagged_with_its_websites_genres_and_has_no_format_unmatched() {
    let fx = Fixture::new();

    let added = fx.add().await;

    assert_eq!(added["tags"], json!(["Action", "drama"]));
    assert_eq!(added["format"], Value::Null);
    let list = json_body(fx.send("GET", "/api/favorites", None).await).await;
    assert_eq!(list[0]["tags"], json!(["Action", "drama"]));
    assert_eq!(list[0]["format"], Value::Null);
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

impl Fixture {
    /// Stores what a new-chapter check of favorite `id` would: the site's chapter count and
    /// links (T65, issue #196).
    fn checked(&self, id: i64, links: &[&str]) {
        let mut favorite = self.db.favorites().get(FavoriteId(id)).unwrap().unwrap();
        favorite.current_chapter = u32::try_from(links.len()).unwrap();
        self.db.favorites().update(&favorite).unwrap();
        self.db
            .favorites()
            .set_chapter_links(FavoriteId(id), links)
            .unwrap();
    }

    async fn new_chapters(&self) -> Value {
        let list = json_body(self.send("GET", "/api/favorites", None).await).await;
        list[0]["new_chapters"].clone()
    }
}

#[tokio::test]
async fn the_badge_counts_the_sites_chapters_that_are_not_marked() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();
    let old: Vec<String> = (1..=50).map(|i| format!("/c{i}")).collect();
    let old: Vec<&str> = old.iter().map(String::as_str).collect();
    fx.db
        .downloaded_chapters()
        .mark("t", "/manga", &old)
        .unwrap();

    // The site drops 5 marked chapters and adds 3: 48 listed, 50 marked.
    let mut site: Vec<&str> = old[5..].to_vec();
    site.extend(["/n1", "/n2", "/n3"]);
    fx.checked(id, &site);

    assert_eq!(fx.new_chapters().await, 3);
}

#[tokio::test]
async fn stale_marks_from_an_old_url_scheme_do_not_hide_new_chapters() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();
    fx.db
        .downloaded_chapters()
        .mark("t", "/manga", &["/old/1", "/old/2", "/old/3", "/old/4"])
        .unwrap();

    // `/c1` and `/c2` were marked when added (case-insensitively, as FMD2 merges the list).
    fx.checked(id, &["/C1", "/c2", "/c3", "/c4", "/c5"]);

    assert_eq!(fx.new_chapters().await, 3);
}

#[tokio::test]
async fn a_favorite_not_checked_since_the_upgrade_keeps_the_count_difference() {
    let fx = Fixture::new();
    // Imported from FMD2, which stores only the chapter count.
    fx.import(5, None);
    fx.db
        .downloaded_chapters()
        .mark("t", "/manga", &["/c1", "/c2"])
        .unwrap();

    // The site's 5 chapters less the 2 marked ones.
    assert_eq!(fx.new_chapters().await, 3);
}

impl Fixture {
    /// Imports `/manga` of module `t` as FMD2 would, with `current_chapter` and `cover_url`
    /// stored.
    fn import(&self, current_chapter: u32, cover_url: Option<&str>) {
        self.db
            .favorites()
            .import(&ImportedFavorite {
                favorite: NewFavorite {
                    module_id: "t".into(),
                    link: "/manga".into(),
                    title: "Manga".into(),
                    save_to: "/data/Manga".into(),
                    cover_url: cover_url.map(Into::into),
                },
                status: "1".into(),
                current_chapter,
                enabled: true,
                date_added: 0,
                date_last_checked: None,
                date_last_updated: None,
            })
            .unwrap();
    }

    async fn cover_url(&self) -> Value {
        let list = json_body(self.send("GET", "/api/favorites", None).await).await;
        list[0]["cover_url"].clone()
    }
}

#[tokio::test]
async fn a_favorite_without_a_stored_cover_gets_the_series_cover_url() {
    let fx = Fixture::new();
    fx.import(0, None);

    assert_eq!(
        fx.cover_url().await,
        "/api/covers/series?module=t&link=%2Fmanga"
    );
}

#[tokio::test]
async fn a_favorite_with_a_stored_cover_gets_the_cover_proxy_url() {
    let fx = Fixture::new();
    fx.import(0, Some("https://example.com/c.jpg"));

    assert_eq!(
        fx.cover_url().await,
        "/api/covers?module=t&url=https%3A%2F%2Fexample.com%2Fc.jpg"
    );
}

#[tokio::test]
async fn groups_are_created_renamed_folded_reordered_and_deleted() {
    let fx = Fixture::new();

    let created = fx
        .send(
            "POST",
            "/api/library/groups",
            Some(json!({ "name": "Shounen" })),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let shounen = json_body(created).await;
    assert_eq!(shounen["name"], "Shounen");
    assert_eq!(shounen["collapsed"], false);
    let seinen = json_body(
        fx.send(
            "POST",
            "/api/library/groups",
            Some(json!({ "name": "Seinen" })),
        )
        .await,
    )
    .await;

    let taken = fx
        .send(
            "POST",
            "/api/library/groups",
            Some(json!({ "name": "SHOUNEN" })),
        )
        .await;
    assert_eq!(taken.status(), StatusCode::CONFLICT);
    let empty = fx
        .send("POST", "/api/library/groups", Some(json!({ "name": "  " })))
        .await;
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);

    let id = shounen["id"].as_i64().unwrap();
    let patched = fx
        .send(
            "PATCH",
            &format!("/api/library/groups/{id}"),
            Some(json!({ "collapsed": true })),
        )
        .await;
    assert_eq!(patched.status(), StatusCode::OK);
    let patched = json_body(patched).await;
    assert_eq!(patched["name"], "Shounen", "a missing name stays");
    assert_eq!(patched["collapsed"], true);
    let renamed = fx
        .send(
            "PATCH",
            &format!("/api/library/groups/{id}"),
            Some(json!({ "name": "Action" })),
        )
        .await;
    assert_eq!(renamed.status(), StatusCode::OK);
    let clash = fx
        .send(
            "PATCH",
            &format!("/api/library/groups/{id}"),
            Some(json!({ "name": "seinen" })),
        )
        .await;
    assert_eq!(clash.status(), StatusCode::CONFLICT);
    let missing = fx
        .send(
            "PATCH",
            "/api/library/groups/999",
            Some(json!({ "collapsed": true })),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let reordered = fx
        .send(
            "POST",
            "/api/library/groups/reorder",
            Some(json!({ "ids": [seinen["id"], id] })),
        )
        .await;
    assert_eq!(reordered.status(), StatusCode::NO_CONTENT);
    let list = json_body(fx.send("GET", "/api/library/groups", None).await).await;
    assert_eq!(
        list,
        json!([
            { "id": seinen["id"], "name": "Seinen", "collapsed": false, "rule": null },
            { "id": id, "name": "Action", "collapsed": true, "rule": null },
        ])
    );

    let deleted = fx
        .send("DELETE", &format!("/api/library/groups/{id}"), None)
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let gone = fx
        .send("DELETE", &format!("/api/library/groups/{id}"), None)
        .await;
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    let list = json_body(fx.send("GET", "/api/library/groups", None).await).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_favorite_is_grouped_kept_in_its_group_and_ungrouped() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();
    let group = json_body(
        fx.send(
            "POST",
            "/api/library/groups",
            Some(json!({ "name": "Shounen" })),
        )
        .await,
    )
    .await;
    let listed_group = |list: Value| list[0]["group_id"].clone();
    assert_eq!(
        listed_group(json_body(fx.send("GET", "/api/favorites", None).await).await),
        Value::Null
    );

    let grouped = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{id}"),
            Some(json!({ "group_id": group["id"] })),
        )
        .await;
    assert_eq!(grouped.status(), StatusCode::OK);
    assert_eq!(
        listed_group(json_body(fx.send("GET", "/api/favorites", None).await).await),
        group["id"]
    );

    let untouched = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{id}"),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(untouched.status(), StatusCode::OK);
    assert_eq!(
        listed_group(json_body(fx.send("GET", "/api/favorites", None).await).await),
        group["id"],
        "a patch without group_id leaves it"
    );

    let unknown = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{id}"),
            Some(json!({ "group_id": 999 })),
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);

    let ungrouped = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{id}"),
            Some(json!({ "group_id": null })),
        )
        .await;
    assert_eq!(ungrouped.status(), StatusCode::OK);
    assert_eq!(
        listed_group(json_body(fx.send("GET", "/api/favorites", None).await).await),
        Value::Null
    );
}

#[tokio::test]
async fn deleting_a_group_lists_its_favorites_ungrouped() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();
    let group = json_body(
        fx.send(
            "POST",
            "/api/library/groups",
            Some(json!({ "name": "Shounen" })),
        )
        .await,
    )
    .await;
    fx.send(
        "PATCH",
        &format!("/api/favorites/{id}"),
        Some(json!({ "group_id": group["id"] })),
    )
    .await;

    let deleted = fx
        .send(
            "DELETE",
            &format!("/api/library/groups/{}", group["id"]),
            None,
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let list = json_body(fx.send("GET", "/api/favorites", None).await).await;
    assert_eq!(list.as_array().unwrap().len(), 1, "the series stays");
    assert_eq!(list[0]["group_id"], Value::Null);
}

impl Fixture {
    /// Favorites in the store named `titles`, each with its own link; their ids.
    fn create(&self, titles: &[&str]) -> Vec<i64> {
        titles
            .iter()
            .map(|title| {
                self.db
                    .favorites()
                    .create(&NewFavorite {
                        module_id: "t".into(),
                        link: format!("/{title}"),
                        title: (*title).into(),
                        save_to: "/old".into(),
                        cover_url: None,
                    })
                    .unwrap()
                    .id
                    .0
            })
            .collect()
    }

    fn stored(&self, id: i64) -> fmd_store::Favorite {
        self.db.favorites().get(FavoriteId(id)).unwrap().unwrap()
    }
}

#[tokio::test]
async fn a_bulk_change_pauses_every_listed_favorite() {
    let fx = Fixture::new();
    let ids = fx.create(&["A", "B", "C"]);

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": [ids[0], ids[2]], "enabled": false })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let updated = json_body(response).await;
    let updated = updated.as_array().unwrap();
    assert_eq!(updated.len(), 2);
    assert_eq!(updated[0]["id"], ids[0]);
    assert_eq!(updated[0]["enabled"], false);
    assert_eq!(updated[1]["id"], ids[2]);
    assert!(!fx.stored(ids[0]).enabled);
    assert!(fx.stored(ids[1]).enabled, "an unlisted favorite stays");
    assert!(!fx.stored(ids[2]).enabled);
}

#[tokio::test]
async fn a_bulk_change_groups_the_listed_favorites() {
    let fx = Fixture::new();
    let ids = fx.create(&["A", "B"]);
    let group = json_body(
        fx.send(
            "POST",
            "/api/library/groups",
            Some(json!({ "name": "Shounen" })),
        )
        .await,
    )
    .await;

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": ids, "group_id": group["id"] })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let list = json_body(fx.send("GET", "/api/favorites", None).await).await;
    assert_eq!(list[0]["group_id"], group["id"]);
    assert_eq!(list[1]["group_id"], group["id"]);

    let ungrouped = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": [ids[0]], "group_id": null })),
        )
        .await;
    assert_eq!(ungrouped.status(), StatusCode::OK);
    let list = json_body(fx.send("GET", "/api/favorites", None).await).await;
    assert_eq!(list[0]["group_id"], Value::Null);
    assert_eq!(list[1]["group_id"], group["id"]);
}

#[tokio::test]
async fn a_bulk_change_with_an_unknown_id_or_group_changes_nothing() {
    let fx = Fixture::new();
    let ids = fx.create(&["A", "B"]);

    let unknown_id = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": [ids[0], 999, ids[1]], "enabled": false })),
        )
        .await;
    assert_eq!(unknown_id.status(), StatusCode::BAD_REQUEST);

    let unknown_group = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": ids, "enabled": false, "group_id": 999 })),
        )
        .await;
    assert_eq!(unknown_group.status(), StatusCode::BAD_REQUEST);

    for id in ids {
        let stored = fx.stored(id);
        assert!(stored.enabled);
        assert_eq!(stored.group_id, None);
    }
}

#[tokio::test]
async fn a_bulk_destination_gives_each_favorite_its_own_manga_folder_in_it() {
    let fx = Fixture::new();
    let ids = fx.create(&["Alpha", "Beta"]);

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": ids, "destination": "/library/new" })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    for (id, title) in ids.into_iter().zip(["Alpha", "Beta"]) {
        let folder = json_body(
            fx.send(
                "POST",
                "/api/save-folder",
                Some(json!({ "module_id": "t", "title": title, "save_to": "/library/new" })),
            )
            .await,
        )
        .await["folder"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(folder.starts_with("/library/new"), "{folder}");
        assert_eq!(fx.stored(id).save_to, folder);
    }
}

#[tokio::test]
async fn a_bulk_removal_removes_every_listed_favorite_or_none() {
    let fx = Fixture::new();
    let ids = fx.create(&["A", "B", "C"]);

    let unknown = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": [ids[0], 999], "remove": true })),
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    assert_eq!(fx.db.favorites().list().unwrap().len(), 3);

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": [ids[0], ids[2]], "remove": true })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, json!([]));
    let left: Vec<i64> = fx
        .db
        .favorites()
        .list()
        .unwrap()
        .into_iter()
        .map(|f| f.id.0)
        .collect();
    assert_eq!(left, [ids[1]]);
}

impl Fixture {
    async fn seen(&self, chapters: &[&str], seen: bool) -> StatusCode {
        self.send(
            "POST",
            "/api/series/seen",
            Some(json!({ "module_id": "t", "link": "/manga", "chapters": chapters, "seen": seen })),
        )
        .await
        .status()
    }

    async fn create_group(&self, body: Value) -> Value {
        let created = self.send("POST", "/api/library/groups", Some(body)).await;
        assert_eq!(created.status(), StatusCode::CREATED);
        json_body(created).await
    }

    async fn listed_group(&self) -> Value {
        json_body(self.send("GET", "/api/favorites", None).await).await[0]["group_id"].clone()
    }
}

#[tokio::test]
async fn marking_chapters_seen_or_unseen_changes_the_badge() {
    let fx = Fixture::new();
    // Adding the series marks its current chapters `/c1` and `/c2` seen.
    let id = fx.add().await["id"].as_i64().unwrap();
    fx.checked(id, &["/c1", "/c2", "/c3", "/c4"]);
    assert_eq!(fx.new_chapters().await, 2);

    assert_eq!(fx.seen(&["/c3"], true).await, StatusCode::NO_CONTENT);
    assert_eq!(fx.new_chapters().await, 1);

    assert_eq!(
        fx.seen(&["/C1", "/c2"], false).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(fx.new_chapters().await, 3);
}

#[tokio::test]
async fn a_series_outside_the_library_can_be_marked_seen() {
    let fx = Fixture::new();

    assert_eq!(fx.seen(&["/c1"], true).await, StatusCode::NO_CONTENT);

    assert_eq!(
        fx.db.downloaded_chapters().list_for("t", "/manga").unwrap(),
        ["/c1"]
    );
}

#[tokio::test]
async fn a_bulk_mark_seen_clears_every_listed_favorites_new_chapters() {
    let fx = Fixture::new();
    let ids = fx.create(&["A", "B", "C"]);
    for id in &ids {
        fx.checked(*id, &["/c1", "/c2"]);
    }

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": [ids[0], ids[2]], "mark_seen": true })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let updated = json_body(response).await;
    assert_eq!(updated[0]["new_chapters"], 0);
    assert_eq!(updated[1]["new_chapters"], 0);
    let list = json_body(fx.send("GET", "/api/favorites", None).await).await;
    let new: Vec<(i64, i64)> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["id"].as_i64().unwrap(),
                f["new_chapters"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(new, [(ids[0], 0), (ids[1], 2), (ids[2], 0)]);
}

#[tokio::test]
async fn a_bulk_mark_seen_cannot_be_combined_with_a_removal() {
    let fx = Fixture::new();
    let ids = fx.create(&["A"]);
    fx.checked(ids[0], &["/c1"]);

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            Some(json!({ "ids": ids, "mark_seen": true, "remove": true })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(fx.db.favorites().list().unwrap().len(), 1);
    assert_eq!(fx.new_chapters().await, 1);
}

#[tokio::test]
async fn creating_a_group_with_a_tag_rule_collects_the_matching_ungrouped_series() {
    let fx = Fixture::new();
    fx.add().await;

    // The series is tagged "Action" and "drama".
    let drama = fx
        .create_group(json!({ "name": "Drama", "rule": { "kind": "tag", "value": " DRAMA " } }))
        .await;

    assert_eq!(drama["rule"], json!({ "kind": "tag", "value": "DRAMA" }));
    assert_eq!(fx.listed_group().await, drama["id"]);
}

#[tokio::test]
async fn patching_a_groups_rule_collects_the_matching_ungrouped_series_and_null_clears_it() {
    let fx = Fixture::new();
    fx.add().await;
    let group = fx.create_group(json!({ "name": "Later" })).await;
    assert_eq!(group["rule"], Value::Null);
    assert_eq!(fx.listed_group().await, Value::Null);
    let uri = format!("/api/library/groups/{}", group["id"]);

    let patched = fx
        .send(
            "PATCH",
            &uri,
            Some(json!({ "rule": { "kind": "tag", "value": "action" } })),
        )
        .await;
    assert_eq!(patched.status(), StatusCode::OK);
    assert_eq!(
        json_body(patched).await["rule"],
        json!({ "kind": "tag", "value": "action" })
    );
    assert_eq!(fx.listed_group().await, group["id"]);

    let folded = fx
        .send("PATCH", &uri, Some(json!({ "collapsed": true })))
        .await;
    assert_eq!(
        json_body(folded).await["rule"],
        json!({ "kind": "tag", "value": "action" }),
        "a missing rule stays"
    );
    let cleared = fx.send("PATCH", &uri, Some(json!({ "rule": null }))).await;
    assert_eq!(cleared.status(), StatusCode::OK);
    assert_eq!(json_body(cleared).await["rule"], Value::Null);
    assert_eq!(
        fx.listed_group().await,
        group["id"],
        "clearing the rule moves nothing"
    );
}

#[tokio::test]
async fn a_rule_with_an_unknown_format_or_an_empty_tag_is_a_bad_request() {
    let fx = Fixture::new();
    let group = fx.create_group(json!({ "name": "Webtoons" })).await;

    for rule in [
        json!({ "kind": "format", "value": "webtoon" }),
        json!({ "kind": "format", "value": "Manhwa" }),
        json!({ "kind": "tag", "value": "  " }),
    ] {
        let created = fx
            .send(
                "POST",
                "/api/library/groups",
                Some(json!({ "name": "Other", "rule": rule })),
            )
            .await;
        assert_eq!(created.status(), StatusCode::BAD_REQUEST, "{rule}");
        let patched = fx
            .send(
                "PATCH",
                &format!("/api/library/groups/{}", group["id"]),
                Some(json!({ "rule": rule })),
            )
            .await;
        assert_eq!(patched.status(), StatusCode::BAD_REQUEST, "{rule}");
    }
    let list = json_body(fx.send("GET", "/api/library/groups", None).await).await;
    assert_eq!(list, json!([group]), "nothing changed");
}

#[tokio::test]
async fn an_added_series_lands_in_the_first_group_whose_rule_matches() {
    let fx = Fixture::new();
    fx.create_group(json!({ "name": "Mine" })).await;
    let action = fx
        .create_group(json!({ "name": "Action", "rule": { "kind": "tag", "value": "Action" } }))
        .await;
    fx.create_group(json!({ "name": "Drama", "rule": { "kind": "tag", "value": "drama" } }))
        .await;

    let added = fx.add().await;

    assert_eq!(added["group_id"], action["id"]);
    assert_eq!(fx.listed_group().await, action["id"]);
}

#[tokio::test]
async fn a_rule_never_moves_a_series_out_of_its_group() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();
    let mine = fx.create_group(json!({ "name": "Mine" })).await;
    fx.send(
        "PATCH",
        &format!("/api/favorites/{id}"),
        Some(json!({ "group_id": mine["id"] })),
    )
    .await;

    let action = fx
        .create_group(json!({ "name": "Action", "rule": { "kind": "tag", "value": "Action" } }))
        .await;
    fx.send(
        "POST",
        "/api/library/groups/reorder",
        Some(json!({ "ids": [action["id"], mine["id"]] })),
    )
    .await;

    assert_eq!(fx.listed_group().await, mine["id"]);
}

#[tokio::test]
async fn reordering_groups_collects_series_moved_to_no_group_by_hand() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();
    let action = fx
        .create_group(json!({ "name": "Action", "rule": { "kind": "tag", "value": "Action" } }))
        .await;
    let drama = fx
        .create_group(json!({ "name": "Drama", "rule": { "kind": "tag", "value": "drama" } }))
        .await;
    fx.send(
        "PATCH",
        &format!("/api/favorites/{id}"),
        Some(json!({ "group_id": null })),
    )
    .await;
    assert_eq!(
        fx.listed_group().await,
        Value::Null,
        "stays without a group"
    );

    let reordered = fx
        .send(
            "POST",
            "/api/library/groups/reorder",
            Some(json!({ "ids": [drama["id"], action["id"]] })),
        )
        .await;

    assert_eq!(reordered.status(), StatusCode::NO_CONTENT);
    assert_eq!(fx.listed_group().await, drama["id"]);
}

#[tokio::test]
async fn saving_a_group_with_its_rule_unchanged_leaves_series_moved_to_no_group() {
    let fx = Fixture::new();
    let id = fx.add().await["id"].as_i64().unwrap();
    let action = fx
        .create_group(json!({ "name": "Action", "rule": { "kind": "tag", "value": "Action" } }))
        .await;
    fx.send(
        "PATCH",
        &format!("/api/favorites/{id}"),
        Some(json!({ "group_id": null })),
    )
    .await;

    // As the Groups dialog's Edit form saves a rename.
    let renamed = fx
        .send(
            "PATCH",
            &format!("/api/library/groups/{}", action["id"]),
            Some(json!({ "name": "Fights", "rule": { "kind": "tag", "value": "Action" } })),
        )
        .await;

    assert_eq!(renamed.status(), StatusCode::OK);
    assert_eq!(fx.listed_group().await, Value::Null);
}
