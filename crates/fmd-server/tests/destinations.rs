//! Repointing library series and websites when a destination's folder changes (#284), driven
//! through `build_router` with `oneshot` against one shared [`AppState`].
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_server::{AppState, build_router};
use fmd_store::{AppDb, FavoriteId, ModuleSettings, NewFavorite};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct Fixture {
    dir: TempDir,
    db: AppDb,
    state: AppState,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let state = AppState::new(db.clone()).unwrap();
        Fixture { dir, db, state }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn text(&self, relative: &str) -> String {
        self.path(relative).to_string_lossy().into_owned()
    }

    /// A library series downloading to `folder` (under the fixture's folder).
    fn series(&self, title: &str, folder: &str) -> i64 {
        self.db
            .favorites()
            .create(&NewFavorite {
                module_id: "t".into(),
                link: format!("/{title}"),
                title: title.into(),
                save_to: self.text(folder),
                cover_url: None,
            })
            .unwrap()
            .id
            .0
    }

    fn save_to(&self, id: i64) -> String {
        self.db
            .favorites()
            .get(FavoriteId(id))
            .unwrap()
            .unwrap()
            .save_to
    }

    /// Website `module`'s own destination is `folder` (under the fixture's folder).
    fn website(&self, module: &str, folder: &str) {
        let mut settings = ModuleSettings::new(module);
        settings.save_to = self.text(folder);
        self.db.module_settings().upsert(&settings).unwrap();
    }

    fn website_save_to(&self, module: &str) -> String {
        self.db
            .module_settings()
            .get(module)
            .unwrap()
            .unwrap()
            .save_to
    }

    async fn relocate(&self, body: Value) -> Response {
        let request = Request::post("/api/destinations/relocate")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        build_router(self.state.clone())
            .oneshot(request)
            .await
            .unwrap()
    }

    async fn get(&self, uri: &str) -> Value {
        let request = Request::get(uri).body(Body::empty()).unwrap();
        json_body(
            build_router(self.state.clone())
                .oneshot(request)
                .await
                .unwrap(),
        )
        .await
    }

    /// The move job's status once it has ended.
    async fn finished_move(&self) -> Value {
        for _ in 0..500 {
            let job = self.get("/api/jobs/move-downloads").await;
            if job["state"] == "done" || job["state"] == "failed" {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the move did not end");
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[tokio::test]
async fn relocating_repoints_the_series_in_and_under_the_old_folder() {
    let fx = Fixture::new();
    let inside = fx.series("Alpha", "manga/Alpha");
    let deeper = fx.series("Beta", "manga/shelf/Beta");
    let itself = fx.series("Gamma", "manga");
    // Shares only a string prefix with the old folder.
    let neighbour = fx.series("Delta", "manga2/Delta");

    let response = fx
        .relocate(json!({ "relocations": [{ "from": fx.path("manga"), "to": fx.path("comics") }] }))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!([{ "from": fx.text("manga"), "to": fx.text("comics"), "series": 3, "websites": 0 }])
    );
    assert_eq!(fx.save_to(inside), fx.text("comics/Alpha"));
    assert_eq!(fx.save_to(deeper), fx.text("comics/shelf/Beta"));
    assert_eq!(fx.save_to(itself), fx.text("comics"));
    assert_eq!(fx.save_to(neighbour), fx.text("manga2/Delta"));
}

#[tokio::test]
async fn relocating_repoints_the_websites_whose_destination_is_the_old_folder() {
    let fx = Fixture::new();
    fx.website("a", "manga");
    fx.website("b", "manga/");
    fx.website("c", "manga/Alpha");
    fx.website("d", "manga2");

    let response = fx
        .relocate(json!({ "relocations": [{ "from": fx.path("manga"), "to": fx.path("comics") }] }))
        .await;

    assert_eq!(
        json_body(response).await,
        json!([{ "from": fx.text("manga"), "to": fx.text("comics"), "series": 0, "websites": 2 }])
    );
    assert_eq!(fx.website_save_to("a"), fx.text("comics"));
    assert_eq!(fx.website_save_to("b"), fx.text("comics"));
    assert_eq!(fx.website_save_to("c"), fx.text("manga/Alpha"));
    assert_eq!(fx.website_save_to("d"), fx.text("manga2"));
}

#[tokio::test]
async fn a_dry_run_counts_what_would_be_repointed_and_changes_nothing() {
    let fx = Fixture::new();
    let alpha = fx.series("Alpha", "manga/Alpha");
    fx.series("Beta", "manga2/Beta");
    fx.website("a", "manga");

    let response = fx
        .relocate(json!({
            "relocations": [{ "from": fx.path("manga"), "to": fx.path("comics") }],
            "dry_run": true,
        }))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!([{ "from": fx.text("manga"), "to": fx.text("comics"), "series": 1, "websites": 1 }])
    );
    assert_eq!(fx.save_to(alpha), fx.text("manga/Alpha"));
    assert_eq!(fx.website_save_to("a"), fx.text("manga"));
}

#[tokio::test]
async fn with_move_files_the_series_chapters_move_to_their_new_folders() {
    let fx = Fixture::new();
    let alpha = fx.series("Alpha", "manga/Alpha");
    // Downloads straight into the destination: only series' folders move, not the destination.
    let gamma = fx.series("Gamma", "manga");
    write(&fx.path("manga/Alpha/Chapter 1.cbz"), "alpha");
    write(&fx.path("manga/notes.txt"), "not a series'");

    let response = fx
        .relocate(json!({
            "relocations": [{ "from": fx.path("manga"), "to": fx.path("comics") }],
            "move_files": true,
        }))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!([{ "from": fx.text("manga"), "to": fx.text("comics"), "series": 2, "websites": 0 }])
    );
    let job = fx.finished_move().await;
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(read(&fx.path("comics/Alpha/Chapter 1.cbz")), "alpha");
    assert!(!fx.path("manga/Alpha").exists());
    assert_eq!(read(&fx.path("manga/notes.txt")), "not a series'");
    assert_eq!(fx.save_to(alpha), fx.text("comics/Alpha"));
    assert_eq!(fx.save_to(gamma), fx.text("comics"));
    let inbox = fx.get("/api/inbox").await;
    assert_eq!(
        inbox[0]["title"], "Moved the downloads of 1 series",
        "{inbox}"
    );
}

#[tokio::test]
async fn an_empty_folder_is_refused_and_changes_nothing() {
    let fx = Fixture::new();
    let alpha = fx.series("Alpha", "manga/Alpha");

    for body in [
        json!({ "relocations": [{ "from": "", "to": fx.path("comics") }] }),
        json!({ "relocations": [{ "from": fx.path("manga"), "to": " " }] }),
    ] {
        let response = fx.relocate(body).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(fx.save_to(alpha), fx.text("manga/Alpha"));
}

#[tokio::test]
async fn several_destinations_are_repointed_and_their_files_moved_in_one_run() {
    let fx = Fixture::new();
    let alpha = fx.series("Alpha", "manga/Alpha");
    let beta = fx.series("Beta", "manhwa/Beta");
    write(&fx.path("manga/Alpha/Chapter 1.cbz"), "alpha");
    write(&fx.path("manhwa/Beta/Chapter 1.cbz"), "beta");

    let response = fx
        .relocate(json!({
            "relocations": [
                { "from": fx.path("manga"), "to": fx.path("new/manga") },
                { "from": fx.path("manhwa"), "to": fx.path("new/manhwa") },
            ],
            "move_files": true,
        }))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let counts = json_body(response).await;
    assert_eq!(
        (counts[0]["series"].clone(), counts[1]["series"].clone()),
        (json!(1), json!(1)),
        "{counts}"
    );
    let job = fx.finished_move().await;
    assert_eq!(
        (job["done"].clone(), job["total"].clone()),
        (json!(2), json!(2))
    );
    assert_eq!(fx.save_to(alpha), fx.text("new/manga/Alpha"));
    assert_eq!(fx.save_to(beta), fx.text("new/manhwa/Beta"));
    assert_eq!(read(&fx.path("new/manga/Alpha/Chapter 1.cbz")), "alpha");
    assert_eq!(read(&fx.path("new/manhwa/Beta/Chapter 1.cbz")), "beta");
}

#[tokio::test]
async fn a_series_in_a_destination_inside_the_old_folder_keeps_its_folder() {
    let fx = Fixture::new();
    // `manga` is a destination of its own inside `library`, whose folder changes.
    let request = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "saveto": { "destinations": [
                { "name": "Library", "path": fx.path("library"), "default": true },
                { "name": "Manga", "path": fx.path("library/manga"), "default": false },
            ] } })
            .to_string(),
        ))
        .unwrap();
    let saved = build_router(fx.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    let alpha = fx.series("Alpha", "library/Alpha");
    let beta = fx.series("Beta", "library/manga/Beta");

    let response = fx
        .relocate(json!({ "relocations": [{ "from": fx.path("library"), "to": fx.path("new") }] }))
        .await;

    assert_eq!(json_body(response).await[0]["series"], 1);
    assert_eq!(fx.save_to(alpha), fx.text("new/Alpha"));
    assert_eq!(fx.save_to(beta), fx.text("library/manga/Beta"));
}
