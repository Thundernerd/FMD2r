//! Queue endpoints (`/api/tasks*`) driven through `build_router` with `oneshot` and a fake
//! download engine that records what it is asked to do
//! (docs/tickets/T23-queue.md, "Seams under test").
//!
//! The operations mirror FMD2's `TDownloadManager` (baseunits/uDownloadsManager.pas:1769-2045).
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::download::{
    ChapterSpec, ChapterStatus, EngineError, EngineEvent, NewDownload, Progress, Task, TaskChapter,
    TaskId, TaskInfo, TaskStatus,
};
use fmd_server::{AppState, DownloadEngine, build_router};
use fmd_store::AppDb;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tower::ServiceExt;

/// A download engine over a fixed queue that records every call.
#[derive(Clone)]
struct FakeEngine {
    tasks: Arc<Mutex<Vec<TaskInfo>>>,
    calls: Arc<Mutex<Vec<String>>>,
    events: broadcast::Sender<EngineEvent>,
}

impl FakeEngine {
    fn new(tasks: Vec<TaskInfo>) -> FakeEngine {
        FakeEngine {
            tasks: Arc::new(Mutex::new(tasks)),
            calls: Arc::default(),
            events: broadcast::channel(16).0,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    /// Records `call` for an existing task, or fails like the engine does for an unknown one.
    fn on_task(&self, id: TaskId, call: &str) -> Result<(), EngineError> {
        if !self.tasks.lock().unwrap().iter().any(|t| t.task.id == id) {
            return Err(EngineError::NoTask(id));
        }
        self.record(format!("{call} {}", id.0));
        Ok(())
    }
}

fn ready<T: Send + 'static>(value: T) -> BoxFuture<'static, T> {
    Box::pin(std::future::ready(value))
}

impl DownloadEngine for FakeEngine {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TaskInfo>, EngineError>> {
        ready(Ok(self.tasks.lock().unwrap().clone()))
    }

    fn add(&self, download: NewDownload) -> BoxFuture<'_, Result<TaskId, EngineError>> {
        if download.module_id != "mangadex" {
            return ready(Err(EngineError::NoModule(download.module_id)));
        }
        if download.chapters.is_empty() {
            return ready(Err(EngineError::NoChapters));
        }
        let mut tasks = self.tasks.lock().unwrap();
        let id = tasks.len() as i64 + 1;
        let mut added = task(id, &download.title, TaskStatus::Waiting, &[]);
        added.chapters = download
            .chapters
            .iter()
            .enumerate()
            .map(|(i, c)| TaskChapter {
                idx: i as u32,
                link: c.link.clone(),
                name: c.title.clone(),
                custom_filename: None,
                status: ChapterStatus::Pending,
                page_count: 0,
                current_page: 0,
            })
            .collect();
        tasks.push(added);
        self.record(format!("add {download:?}"));
        ready(Ok(TaskId(id)))
    }

    fn start(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(self.on_task(id, "start"))
    }

    fn stop(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(self.on_task(id, "stop"))
    }

    fn redownload(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(self.on_task(id, "redownload"))
    }

    fn enable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(self.on_task(id, "enable"))
    }

    fn disable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(self.on_task(id, "disable"))
    }

    fn start_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        self.record("start-all".into());
        ready(Ok(()))
    }

    fn stop_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        self.record("stop-all".into());
        ready(Ok(()))
    }

    fn delete(&self, id: TaskId, files: bool) -> BoxFuture<'_, Result<(), EngineError>> {
        let result = self.on_task(id, &format!("delete files={files}"));
        self.tasks.lock().unwrap().retain(|t| t.task.id != id);
        ready(result)
    }

    fn reorder(&self, ids: Vec<TaskId>) -> BoxFuture<'_, Result<(), EngineError>> {
        let ids: Vec<i64> = ids.iter().map(|id| id.0).collect();
        self.record(format!("reorder {ids:?}"));
        ready(Ok(()))
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<EngineEvent>> {
        Some(self.events.subscribe())
    }
}

/// A stored task in the fake queue.
fn task(
    id: i64,
    title: &str,
    status: TaskStatus,
    chapters: &[(&str, ChapterStatus, u32, u32)],
) -> TaskInfo {
    TaskInfo {
        task: Task {
            id: TaskId(id),
            module_id: "mangadex".into(),
            link: format!("/title/{id}"),
            title: title.into(),
            save_to: format!("/downloads/{title}"),
            status,
            enabled: status != TaskStatus::Disabled,
            sort_order: id,
            // 2026-01-0<id>T00:00:00Z
            date_added: 1_767_225_600_000 + (id - 1) * 86_400_000,
            date_last_downloaded: None,
            current_chapter: 0,
            error: None,
        },
        chapters: chapters
            .iter()
            .enumerate()
            .map(|(i, (name, status, pages, done))| TaskChapter {
                idx: i as u32,
                link: format!("/chapter/{id}/{i}"),
                name: (*name).into(),
                custom_filename: None,
                status: *status,
                page_count: *pages,
                current_page: *done,
            })
            .collect(),
        running: false,
        progress: None,
    }
}

/// A queue with one task in each status group.
fn queue() -> Vec<TaskInfo> {
    let mut downloading = task(
        1,
        "One Piece",
        TaskStatus::Downloading,
        &[
            ("Ch. 1100", ChapterStatus::Downloaded, 17, 17),
            ("Ch. 1101", ChapterStatus::Pending, 0, 0),
        ],
    );
    downloading.task.current_chapter = 1;
    downloading.running = true;
    downloading.progress = Some(Progress {
        task: TaskId(1),
        chapter: 1,
        pages_done: 5,
        pages_total: 20,
        bytes: 4_000_000,
        bytes_per_sec: 250_000,
    });
    let waiting = task(
        2,
        "Berserk",
        TaskStatus::Waiting,
        &[("Vol. 42", ChapterStatus::Pending, 0, 0)],
    );
    let mut failed = task(
        3,
        "Vagabond",
        TaskStatus::Failed,
        &[("Ch. 1", ChapterStatus::Failed, 30, 12)],
    );
    failed.task.error = Some("page 13 missing".into());
    let mut finished = task(
        4,
        "Blame!",
        TaskStatus::Finished,
        &[("Ch. 1", ChapterStatus::Downloaded, 40, 40)],
    );
    finished.task.date_last_downloaded = Some(1_767_312_000_000);
    vec![downloading, waiting, failed, finished]
}

struct Harness {
    _dir: TempDir,
    state: AppState,
    engine: FakeEngine,
}

fn harness(tasks: Vec<TaskInfo>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let engine = FakeEngine::new(tasks);
    Harness {
        _dir: dir,
        state: AppState::new(db).unwrap().with_engine(engine.clone()),
        engine,
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn post(uri: &str) -> Request<Body> {
    Request::post(uri).body(Body::empty()).unwrap()
}

fn delete(uri: &str) -> Request<Body> {
    Request::delete(uri).body(Body::empty()).unwrap()
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::post(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn body_json(res: Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn lists_tasks_in_queue_order_with_progress_and_group_counts() {
    let h = harness(queue());
    let res = send(&h.state, get("/api/tasks")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let titles: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["One Piece", "Berserk", "Vagabond", "Blame!"]);
    assert_eq!(body["total"], 4);
    assert_eq!(
        body["counts"],
        json!({"downloading": 1, "waiting": 1, "stopped": 1, "finished": 1})
    );
    let running = &body["items"][0];
    assert_eq!(running["id"], 1);
    assert_eq!(running["module_id"], "mangadex");
    assert_eq!(running["link"], "/title/1");
    assert_eq!(running["status"], "downloading");
    assert_eq!(running["running"], true);
    // The current chapter's pages, as FMD2's Progress column shows them.
    assert_eq!(running["chapters"], "Ch. 1101 (2/2)");
    assert_eq!(running["chapter_count"], 2);
    assert_eq!(running["chapters_done"], 1);
    assert_eq!(running["done"], 5);
    assert_eq!(running["total"], 20);
    assert_eq!(running["bytes_per_sec"], 250_000.0);
    assert_eq!(running["date_added"], "2026-01-01T00:00:00Z");
    let failed = &body["items"][2];
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"], "page 13 missing");
    assert_eq!(failed["chapters"], "Ch. 1");
    assert_eq!(failed["done"], 12);
    assert_eq!(failed["total"], 30);
    assert_eq!(failed["bytes_per_sec"], 0.0);
    assert_eq!(
        body["items"][3]["date_last_downloaded"],
        "2026-01-02T00:00:00Z"
    );
}

async fn titles(state: &AppState, uri: &str) -> (Vec<String>, Value) {
    let res = send(state, get(uri)).await;
    assert_eq!(res.status(), StatusCode::OK, "{uri}");
    let body = body_json(res).await;
    let titles = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_owned())
        .collect();
    (titles, body)
}

#[tokio::test]
async fn filters_by_status_group_text_and_date_range() {
    let h = harness(queue());
    // Failed, stopped and disabled tasks share a group.
    let (found, body) = titles(&h.state, "/api/tasks?status=stopped").await;
    assert_eq!(found, ["Vagabond"]);
    assert_eq!(body["total"], 1);
    // Group counts ignore the status filter, so every group header keeps its count.
    assert_eq!(
        body["counts"],
        json!({"downloading": 1, "waiting": 1, "stopped": 1, "finished": 1})
    );
    // Text matches the title, the module or a chapter name, ignoring case.
    assert_eq!(titles(&h.state, "/api/tasks?q=BLAME").await.0, ["Blame!"]);
    assert_eq!(
        titles(&h.state, "/api/tasks?q=vol.%2042").await.0,
        ["Berserk"]
    );
    let (found, body) = titles(&h.state, "/api/tasks?q=ch.%201&status=finished").await;
    assert_eq!(found, ["Blame!"]);
    assert_eq!(
        body["counts"],
        json!({"downloading": 1, "waiting": 0, "stopped": 1, "finished": 1})
    );
    // Dates are the last download, or when added for a task never downloaded: Jan 2 to 3.
    let (found, _) = titles(&h.state, "/api/tasks?from=1767312000000&to=1767398400000").await;
    assert_eq!(found, ["Berserk", "Vagabond", "Blame!"]);
}

#[tokio::test]
async fn pages_through_the_queue() {
    let h = harness(queue());
    let (found, body) = titles(&h.state, "/api/tasks?per_page=3&page=2").await;
    assert_eq!(found, ["Blame!"]);
    assert_eq!(body["total"], 4);
    assert_eq!(body["page"], 2);
    assert_eq!(body["per_page"], 3);
    let (found, _) = titles(&h.state, "/api/tasks?per_page=3&page=3").await;
    assert!(found.is_empty());
    let res = send(&h.state, get("/api/tasks?page=0")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// FMD2 sorts the downloads list by a column, comparing text naturally and dates by time
/// (`CompareTaskContainer`, baseunits/uDownloadsManager.pas:2046-2099).
#[tokio::test]
async fn sorts_by_fmd2_columns() {
    let h = harness(queue());
    assert_eq!(
        titles(&h.state, "/api/tasks?sort=title").await.0,
        ["Berserk", "Blame!", "One Piece", "Vagabond"]
    );
    assert_eq!(
        titles(&h.state, "/api/tasks?sort=title&desc=true").await.0,
        ["Vagabond", "One Piece", "Blame!", "Berserk"]
    );
    assert_eq!(
        titles(&h.state, "/api/tasks?sort=added&desc=true").await.0,
        ["Blame!", "Vagabond", "Berserk", "One Piece"]
    );
    assert_eq!(
        titles(&h.state, "/api/tasks?sort=status").await.0,
        ["One Piece", "Vagabond", "Blame!", "Berserk"]
    );
}

#[tokio::test]
async fn a_task_shows_its_chapters_with_per_chapter_progress() {
    let h = harness(queue());
    let res = send(&h.state, get("/api/tasks/1")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["task"]["title"], "One Piece");
    assert_eq!(
        body["chapters"],
        json!([
            {"index": 0, "name": "Ch. 1100", "link": "/chapter/1/0", "status": "downloaded",
             "done": 17, "total": 17},
            // The running chapter shows the live page count.
            {"index": 1, "name": "Ch. 1101", "link": "/chapter/1/1", "status": "pending",
             "done": 5, "total": 20},
        ])
    );
    let res = send(&h.state, get("/api/tasks/99")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
}

/// `AddToDownload` (mangadownloader/forms/frmMain.pas:2653-2790) queues the selected chapters,
/// numbered by their position in the series' chapter list.
#[tokio::test]
async fn adding_a_task_queues_the_selected_chapters() {
    let h = harness(Vec::new());
    let body = json!({
        "module_id": "mangadex",
        "link": "/title/abc",
        "title": "Dandadan",
        "authors": "Tatsu Yukinobu",
        "chapters": [
            {"name": "Ch. 2", "link": "/chapter/2", "number": 2},
            {"name": "Ch. 3", "link": "/chapter/3", "number": 3},
        ],
        "save_to": "/data/manga",
    });
    let res = send(&h.state, post_json("/api/tasks", body)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let task = body_json(res).await;
    assert_eq!(task["id"], 1);
    assert_eq!(task["title"], "Dandadan");
    assert_eq!(task["status"], "waiting");
    assert_eq!(task["chapter_count"], 2);
    let expected = NewDownload {
        module_id: "mangadex".into(),
        manga_link: "/title/abc".into(),
        title: "Dandadan".into(),
        authors: "Tatsu Yukinobu".into(),
        artists: String::new(),
        chapters: vec![
            ChapterSpec {
                link: "/chapter/2".into(),
                title: "Ch. 2".into(),
                number: 2,
            },
            ChapterSpec {
                link: "/chapter/3".into(),
                title: "Ch. 3".into(),
                number: 3,
            },
        ],
        save_to: "/data/manga".into(),
    };
    assert_eq!(h.engine.calls(), [format!("add {expected:?}")]);
}

#[tokio::test]
async fn adding_without_a_save_to_uses_the_default_folder_and_numbers_by_position() {
    let h = harness(Vec::new());
    let body = json!({
        "module_id": "mangadex",
        "link": "/title/abc",
        "title": "Dandadan",
        "chapters": [{"name": "Ch. 1", "link": "/chapter/1"}],
    });
    let res = send(&h.state, post_json("/api/tasks", body)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let expected = NewDownload {
        module_id: "mangadex".into(),
        manga_link: "/title/abc".into(),
        title: "Dandadan".into(),
        chapters: vec![ChapterSpec {
            link: "/chapter/1".into(),
            title: "Ch. 1".into(),
            number: 1,
        }],
        ..NewDownload::default()
    };
    assert_eq!(h.engine.calls(), [format!("add {expected:?}")]);
}

#[tokio::test]
async fn adding_an_invalid_task_is_a_422_naming_the_field() {
    let h = harness(Vec::new());
    let no_chapters = json!({"module_id": "mangadex", "link": "/t", "title": "T", "chapters": []});
    let res = send(&h.state, post_json("/api/tasks", no_chapters)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(res).await["field"], "chapters");
    let unknown = json!({"module_id": "nope", "link": "/t", "title": "T",
        "chapters": [{"name": "Ch. 1", "link": "/c/1"}]});
    let res = send(&h.state, post_json("/api/tasks", unknown)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(res).await["field"], "module_id");
}

#[tokio::test]
async fn task_actions_drive_the_engine_and_answer_the_task() {
    let h = harness(queue());
    for action in ["start", "stop", "redownload", "enable", "disable"] {
        let res = send(&h.state, post(&format!("/api/tasks/3/{action}"))).await;
        assert_eq!(res.status(), StatusCode::OK, "{action}");
        assert_eq!(body_json(res).await["id"], 3, "{action}");
    }
    assert_eq!(
        h.engine.calls(),
        ["start 3", "stop 3", "redownload 3", "enable 3", "disable 3"]
    );
    let res = send(&h.state, post("/api/tasks/99/start")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn start_all_stop_all_and_reorder_drive_the_engine() {
    let h = harness(queue());
    let res = send(&h.state, post("/api/tasks/start-all")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(&h.state, post("/api/tasks/stop-all")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &h.state,
        post_json("/api/tasks/reorder", json!({"ids": [3, 1]})),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        h.engine.calls(),
        ["start-all", "stop-all", "reorder [3, 1]"]
    );
}

#[tokio::test]
async fn deleting_a_task_keeps_its_files_unless_asked() {
    let h = harness(queue());
    let res = send(&h.state, delete("/api/tasks/2")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(&h.state, delete("/api/tasks/3?files=true")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        h.engine.calls(),
        ["delete files=false 2", "delete files=true 3"]
    );
    let res = send(&h.state, delete("/api/tasks/3")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// `RemoveAllFinishedTasks` (baseunits/uDownloadsManager.pas:1987-2001) deletes the finished
/// tasks and keeps their files.
#[tokio::test]
async fn removing_finished_tasks_deletes_only_those_and_keeps_their_files() {
    let h = harness(queue());
    let res = send(&h.state, delete("/api/tasks?status=finished")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(h.engine.calls(), ["delete files=false 4"]);
    // Only finished tasks can be cleared in bulk.
    let res = send(&h.state, delete("/api/tasks?status=waiting")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = send(&h.state, delete("/api/tasks")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// A finished task whose chapters are saved in `dir`.
fn finished_in(dir: &Path, title: &str, chapters: &[&str]) -> TaskInfo {
    let chapters: Vec<_> = chapters
        .iter()
        .map(|name| (*name, ChapterStatus::Downloaded, 2, 2))
        .collect();
    let mut info = task(1, title, TaskStatus::Finished, &chapters);
    info.task.save_to = dir.to_string_lossy().into_owned();
    info
}

async fn body_bytes(res: Response) -> Vec<u8> {
    res.into_body().collect().await.unwrap().to_bytes().to_vec()
}

/// The names and contents of the files in a zip.
fn unzip(bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut files = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        if file.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        files.push((file.name().to_owned(), data));
    }
    files.sort();
    files
}

#[tokio::test]
async fn get_files_streams_a_single_archive_with_its_media_type() {
    for (ext, mime) in [
        ("cbz", "application/vnd.comicbook+zip"),
        ("zip", "application/zip"),
        ("pdf", "application/pdf"),
        ("epub", "application/epub+zip"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(format!("Ch. 1.{ext}")), b"packed chapter").unwrap();
        let h = harness(vec![finished_in(dir.path(), "Blame!", &["Ch. 1"])]);
        let res = send(&h.state, get("/api/tasks/1/files")).await;
        assert_eq!(res.status(), StatusCode::OK, "{ext}");
        assert_eq!(res.headers()["content-type"], mime);
        assert_eq!(
            res.headers()["content-disposition"],
            format!("attachment; filename=\"Ch. 1.{ext}\"; filename*=UTF-8''Ch.%201.{ext}")
                .as_str()
        );
        assert_eq!(res.headers()["content-length"], "14");
        assert_eq!(body_bytes(res).await, b"packed chapter");
    }
}

#[tokio::test]
async fn get_files_zips_several_chapters_named_after_the_series() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ch. 1.cbz"), b"one").unwrap();
    std::fs::write(dir.path().join("Ch. 2.cbz"), b"two").unwrap();
    let h = harness(vec![finished_in(
        dir.path(),
        "Kaiju №8",
        &["Ch. 1", "Ch. 2", "Ch. 3"],
    )]);
    let res = send(&h.state, get("/api/tasks/1/files")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "application/zip");
    // Non-ASCII names fall back to `_` in the plain filename (RFC 6266).
    assert_eq!(
        res.headers()["content-disposition"],
        "attachment; filename=\"Kaiju _8.zip\"; filename*=UTF-8''Kaiju%20%E2%84%968.zip"
    );
    // Chapter 3 has no file yet; it is left out.
    assert_eq!(
        unzip(body_bytes(res).await),
        [
            ("Ch. 1.cbz".into(), b"one".to_vec()),
            ("Ch. 2.cbz".into(), b"two".to_vec())
        ]
    );
}

#[tokio::test]
async fn get_files_zips_folder_output() {
    // Chapter folders.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Ch. 1")).unwrap();
    std::fs::write(dir.path().join("Ch. 1/001.png"), b"p1").unwrap();
    std::fs::write(dir.path().join("Ch. 1/002.png"), b"p2").unwrap();
    let h = harness(vec![finished_in(dir.path(), "Blame!", &["Ch. 1"])]);
    let res = send(&h.state, get("/api/tasks/1/files")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "application/zip");
    assert_eq!(
        unzip(body_bytes(res).await),
        [
            ("Ch. 1/001.png".into(), b"p1".to_vec()),
            ("Ch. 1/002.png".into(), b"p2".to_vec())
        ]
    );

    // Pages saved straight into the series folder (no chapter folders).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ch. 1 001.png"), b"p1").unwrap();
    let h = harness(vec![finished_in(dir.path(), "Blame!", &["Ch. 1"])]);
    let res = send(&h.state, get("/api/tasks/1/files")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        unzip(body_bytes(res).await),
        [("Ch. 1 001.png".into(), b"p1".to_vec())]
    );
}

#[tokio::test]
async fn get_files_without_files_on_disk_is_a_404() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");
    let h = harness(vec![finished_in(&missing, "Blame!", &["Ch. 1"])]);
    let res = send(&h.state, get("/api/tasks/1/files")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
    let res = send(&h.state, get("/api/tasks/2/files")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
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
    tokio::time::timeout(std::time::Duration::from_secs(5), read)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {needle:?}; got {seen:?}"));
    seen
}

/// The (event, data) pairs of the SSE frames in `text`, heartbeats left out.
fn frames(text: &str) -> Vec<(String, Value)> {
    text.split("\n\n")
        .filter_map(|frame| {
            let event = frame.lines().find_map(|l| l.strip_prefix("event: "))?;
            let data = frame.lines().find_map(|l| l.strip_prefix("data: "))?;
            Some((event.to_owned(), serde_json::from_str(data).unwrap()))
        })
        .collect()
}

fn progress(done: u32, total: u32) -> EngineEvent {
    EngineEvent::Progress(Progress {
        task: TaskId(1),
        chapter: 1,
        pages_done: done,
        pages_total: total,
        bytes: u64::from(done) * 100_000,
        bytes_per_sec: 300_000,
    })
}

/// Engine events reach `/api/events` as `task.status` and `task.progress` frames, progress at
/// most 4 times a second per task, as FMD2 refreshes its downloads view on a timer
/// (`tmRefreshDownloadsInfoTimer`, mangadownloader/forms/frmMain.pas:2015-2028).
#[tokio::test]
async fn engine_events_are_streamed_as_task_frames_with_progress_throttled() {
    let h = harness(queue());
    let res = send(&h.state, get("/api/events")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let events = &h.engine.events;
    events
        .send(EngineEvent::Status {
            task: TaskId(1),
            status: TaskStatus::Downloading,
            chapter: 1,
            error: None,
        })
        .unwrap();
    events.send(progress(6, 20)).unwrap();
    // Too soon after the last one: dropped.
    events.send(progress(7, 20)).unwrap();
    // The chapter's last page always goes through.
    events.send(progress(20, 20)).unwrap();
    events
        .send(EngineEvent::Status {
            task: TaskId(3),
            status: TaskStatus::Failed,
            chapter: 0,
            error: Some("page 13 missing".into()),
        })
        .unwrap();
    events.send(EngineEvent::Reordered).unwrap();
    events
        .send(EngineEvent::Deleted { task: TaskId(2) })
        .unwrap();

    let seen = frames(&read_sse_until(res, "event: task.removed").await);
    assert_eq!(
        seen,
        [
            (
                "task.status".into(),
                json!({"id": 1, "status": "downloading", "error": null})
            ),
            (
                "task.progress".into(),
                json!({"id": 1, "title": "One Piece",
                "chapters": "Ch. 1101 (2/2)", "status": "downloading", "done": 6, "total": 20,
                "bytes_per_sec": 300_000.0})
            ),
            (
                "task.progress".into(),
                json!({"id": 1, "title": "One Piece",
                "chapters": "Ch. 1101 (2/2)", "status": "downloading", "done": 20, "total": 20,
                "bytes_per_sec": 300_000.0})
            ),
            (
                "task.status".into(),
                json!({"id": 3, "status": "failed", "error": "page 13 missing"})
            ),
            ("task.reordered".into(), json!({})),
            ("task.removed".into(), json!({"id": 2})),
        ]
    );
}
