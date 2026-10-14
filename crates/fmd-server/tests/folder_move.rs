//! Moving a library series' downloaded chapters along with a folder change (#283), driven
//! through `build_router` with `oneshot` against one shared [`AppState`].
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::download::{EngineError, NewDownload, Task, TaskId, TaskInfo, TaskStatus};
use fmd_core::modules::ModuleInfo;
use fmd_server::{AppState, DownloadEngine, ModuleCatalog, ModulesReport, build_router};
use fmd_store::{AppDb, FavoriteId, NewFavorite};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// Module `t` ("T").
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
}

/// One task downloading to `folder` while `running`, as a running download of a series.
#[derive(Clone)]
struct Engine {
    folder: String,
    running: Arc<AtomicBool>,
}

impl Engine {
    fn downloading_to(folder: &Path) -> Engine {
        Engine {
            folder: folder.to_string_lossy().into_owned(),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    fn finish(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn ok() -> BoxFuture<'static, Result<(), EngineError>> {
    Box::pin(std::future::ready(Ok(())))
}

impl DownloadEngine for Engine {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TaskInfo>, EngineError>> {
        let task = TaskInfo {
            task: Task {
                id: TaskId(1),
                module_id: "t".into(),
                link: "/Alpha".into(),
                title: "Alpha".into(),
                save_to: self.folder.clone(),
                status: TaskStatus::Downloading,
                enabled: true,
                sort_order: 0,
                date_added: 0,
                date_last_downloaded: None,
                current_chapter: 0,
                error: None,
            },
            chapters: Vec::new(),
            running: self.running.load(Ordering::SeqCst),
            progress: None,
        };
        Box::pin(std::future::ready(Ok(vec![task])))
    }

    fn add(&self, _: NewDownload) -> BoxFuture<'_, Result<TaskId, EngineError>> {
        Box::pin(std::future::ready(Ok(TaskId(2))))
    }

    fn start(&self, _: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn stop(&self, _: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn redownload(&self, _: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn enable(&self, _: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn disable(&self, _: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn start_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn stop_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn delete(&self, _: TaskId, _: bool) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }

    fn reorder(&self, _: Vec<TaskId>) -> BoxFuture<'_, Result<(), EngineError>> {
        ok()
    }
}

struct Fixture {
    dir: TempDir,
    db: AppDb,
    state: AppState,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let state = AppState::new(db.clone()).unwrap().with_modules(Modules);
        Fixture { dir, db, state }
    }

    fn with_engine(mut self, engine: Engine) -> Fixture {
        self.state = self.state.with_engine(engine);
        self
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// A library series downloading to `folder` (under the fixture's folder).
    fn series(&self, title: &str, folder: &str) -> i64 {
        self.db
            .favorites()
            .create(&NewFavorite {
                module_id: "t".into(),
                link: format!("/{title}"),
                title: title.into(),
                save_to: self.path(folder).to_string_lossy().into_owned(),
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

    async fn send(&self, method: &str, uri: &str, body: Value) -> Response {
        let request = Request::builder()
            .method(method)
            .uri(uri)
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
async fn changing_a_series_folder_with_move_files_moves_its_chapters_there() {
    let fx = Fixture::new();
    let id = fx.series("Alpha", "old/Alpha");
    write(&fx.path("old/Alpha/Chapter 1.cbz"), "chapter 1");
    let new = fx.path("new/Alpha");

    let response = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{id}"),
            json!({ "save_to": new, "move_files": true }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(fx.save_to(id), new.to_string_lossy());
    let job = fx.finished_move().await;
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(
        (job["done"].clone(), job["total"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(read(&new.join("Chapter 1.cbz")), "chapter 1");
    assert!(!fx.path("old/Alpha").exists());
    let inbox = fx.get("/api/inbox").await;
    assert_eq!(
        inbox[0]["title"], "Moved the downloads of 1 series",
        "{inbox}"
    );
    assert_eq!(inbox[0]["kind"], "info", "{inbox}");
}

#[tokio::test]
async fn a_bulk_destination_with_move_files_moves_each_selected_series() {
    let fx = Fixture::new();
    let alpha = fx.series("Alpha", "old/Alpha");
    let beta = fx.series("Beta", "old/Beta");
    let gamma = fx.series("Gamma", "old/Gamma");
    for title in ["Alpha", "Beta", "Gamma"] {
        write(&fx.path(&format!("old/{title}/Chapter 1.cbz")), title);
    }
    let destination = fx.path("new");

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            json!({ "ids": [alpha, beta], "destination": destination, "move_files": true }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let job = fx.finished_move().await;
    assert_eq!(
        (job["done"].clone(), job["total"].clone()),
        (json!(2), json!(2))
    );
    for (id, title) in [(alpha, "Alpha"), (beta, "Beta")] {
        let folder = PathBuf::from(fx.save_to(id));
        assert!(folder.starts_with(&destination), "{}", folder.display());
        assert_eq!(read(&folder.join("Chapter 1.cbz")), title);
        assert!(!fx.path(&format!("old/{title}")).exists());
    }
    assert_eq!(read(&fx.path("old/Gamma/Chapter 1.cbz")), "Gamma");
    assert_eq!(fx.save_to(gamma), fx.path("old/Gamma").to_string_lossy());
    let inbox = fx.get("/api/inbox").await;
    assert_eq!(
        inbox[0]["title"], "Moved the downloads of 2 series",
        "{inbox}"
    );
}

#[tokio::test]
async fn a_second_move_while_one_runs_is_a_conflict_and_changes_nothing() {
    let fx = Fixture::new();
    let engine = Engine::downloading_to(&fx.path("old/Alpha"));
    let fx = fx.with_engine(engine.clone());
    let alpha = fx.series("Alpha", "old/Alpha");
    let beta = fx.series("Beta", "old/Beta");
    write(&fx.path("old/Alpha/Chapter 1.cbz"), "alpha");
    write(&fx.path("old/Beta/Chapter 1.cbz"), "beta");
    let first = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{alpha}"),
            json!({ "save_to": fx.path("new/Alpha"), "move_files": true }),
        )
        .await;
    assert_eq!(first.status(), StatusCode::OK);

    // Alpha's move waits for its running download.
    let second = fx
        .send(
            "PATCH",
            &format!("/api/favorites/{beta}"),
            json!({ "save_to": fx.path("new/Beta"), "move_files": true }),
        )
        .await;

    assert_eq!(second.status(), StatusCode::CONFLICT);
    let problem = json_body(second).await;
    assert!(
        problem["detail"]
            .as_str()
            .unwrap()
            .contains("Move downloads"),
        "{problem}"
    );
    assert_eq!(fx.save_to(beta), fx.path("old/Beta").to_string_lossy());
    assert_eq!(read(&fx.path("old/Alpha/Chapter 1.cbz")), "alpha");
    let job = fx.get("/api/jobs/move-downloads").await;
    assert_eq!(job["state"], "running", "{job}");

    engine.finish();
    fx.finished_move().await;
    assert_eq!(read(&fx.path("new/Alpha/Chapter 1.cbz")), "alpha");
    assert_eq!(read(&fx.path("old/Beta/Chapter 1.cbz")), "beta");
}

#[tokio::test]
async fn move_files_cannot_be_combined_with_a_removal() {
    let fx = Fixture::new();
    let alpha = fx.series("Alpha", "old/Alpha");

    let response = fx
        .send(
            "POST",
            "/api/favorites/bulk",
            json!({ "ids": [alpha], "remove": true, "move_files": true }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(fx.db.favorites().list().unwrap().len(), 1);
}

#[tokio::test]
async fn skipped_files_and_series_are_reported_as_a_warning() {
    let fx = Fixture::new();
    let alpha = fx.series("Alpha", "old/Alpha");
    // Downloaded straight into a folder another series' folder is in.
    let beta = fx.series("Beta", "shared");
    fx.series("Gamma", "shared/Gamma");
    write(&fx.path("old/Alpha/Chapter 1.cbz"), "old alpha");
    write(&fx.path("new/Alpha/Chapter 1.cbz"), "new alpha");
    write(&fx.path("shared/Gamma/Chapter 1.cbz"), "gamma");

    for (id, to) in [(alpha, "new/Alpha"), (beta, "new/Beta")] {
        let response = fx
            .send(
                "PATCH",
                &format!("/api/favorites/{id}"),
                json!({ "save_to": fx.path(to), "move_files": true }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        fx.finished_move().await;
    }

    assert_eq!(read(&fx.path("new/Alpha/Chapter 1.cbz")), "new alpha");
    assert_eq!(read(&fx.path("old/Alpha/Chapter 1.cbz")), "old alpha");
    assert_eq!(read(&fx.path("shared/Gamma/Chapter 1.cbz")), "gamma");
    let inbox = fx.get("/api/inbox").await;
    let (beta_item, alpha_item) = (&inbox[0], &inbox[1]);
    assert_eq!(alpha_item["kind"], "warn", "{inbox}");
    let body = alpha_item["body"].as_str().unwrap();
    assert!(
        body.contains("Alpha: skipped") && body.contains("Chapter 1.cbz"),
        "{body}"
    );
    assert_eq!(
        beta_item["title"], "Moved the downloads of 0 series",
        "{inbox}"
    );
    assert_eq!(beta_item["kind"], "warn", "{inbox}");
    let body = beta_item["body"].as_str().unwrap();
    assert!(body.contains("Beta: skipped the series"), "{body}");
}
