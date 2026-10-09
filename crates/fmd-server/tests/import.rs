//! `POST /api/import` driven through `build_router` with `oneshot`, uploading zipped FMD2
//! `userdata` directories built with T32's fixtures
//! (docs/tickets/T47-import-upload-and-timezone.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "../../fmd-import/tests/common/mod.rs"]
mod fmd2;

use std::io::Write;
use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fmd_core::download::{EngineError, NewDownload, TaskId, TaskInfo};
use fmd_server::{AppState, DownloadEngine, ImportLimits, build_router};
use fmd_store::AppDb;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;
use zip::write::SimpleFileOptions;

/// A server over a fresh data directory.
struct Server {
    _dir: TempDir,
    db: AppDb,
    state: AppState,
}

impl Server {
    fn new() -> Self {
        Self::with(|state| state)
    }

    fn with(configure: impl FnOnce(AppState) -> AppState) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let state = configure(AppState::new(db.clone()).unwrap().with_data_dir(dir.path()));
        Self {
            _dir: dir,
            db,
            state,
        }
    }

    /// The upload folders left in the data directory.
    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(self._dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with(".import-"))
            .collect()
    }

    /// `POST /api/import?<query>` with `zip` as the body.
    async fn import(&self, query: &str, zip: Vec<u8>) -> (StatusCode, Value) {
        let response = build_router(self.state.clone())
            .oneshot(
                Request::post(format!("/api/import?{query}"))
                    .header("content-type", "application/zip")
                    .body(Body::from(zip))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }
}

impl Server {
    async fn get(&self, uri: &str) -> Value {
        let response = build_router(self.state.clone())
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "GET {uri}");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).unwrap()
    }

    /// Waits until an import is running.
    async fn wait_for_import_job(&self) {
        loop {
            let response = build_router(self.state.clone())
                .oneshot(
                    Request::get("/api/jobs/import")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            if response.status() == StatusCode::OK {
                let body = response.into_body().collect().await.unwrap().to_bytes();
                let job: Value = serde_json::from_slice(&body).unwrap();
                if job["state"] == "running" {
                    return;
                }
            }
            tokio::task::yield_now().await;
        }
    }
}

/// The files of `dir` zipped under `prefix` (e.g. `userdata/`), deflated as zip tools do.
fn zip_dir(dir: &Path, prefix: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = format!("{prefix}{}", entry.file_name().to_str().unwrap());
        zip.start_file(name, options).unwrap();
        zip.write_all(&std::fs::read(entry.path()).unwrap())
            .unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[tokio::test]
async fn a_dry_run_reports_what_would_be_imported_and_writes_nothing() {
    let server = Server::new();
    let userdata = fmd2::every_source();

    let (status, report) = server
        .import("dry_run=true", zip_dir(userdata.dir(), "userdata/"))
        .await;

    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["tasks"]["imported"], 2);
    assert_eq!(report["favorites"]["imported"], 1);
    assert!(server.db.tasks().list().unwrap().is_empty());
    assert!(server.db.favorites().list().unwrap().is_empty());
}

#[tokio::test]
async fn an_import_gives_the_same_report_as_the_cli() {
    let server = Server::new();
    let userdata = fmd2::every_source();
    let opts = fmd_import::ImportOptions {
        resume_in_progress: true,
        path_maps: vec!["C:\\Manga=/data/manga".parse().unwrap()],
        ..fmd_import::ImportOptions::default()
    };
    // What `fmd2r import --resume --map-path 'C:\Manga=/data/manga'` reports for the same folder.
    let cli = fmd2::App::new().import_with(&userdata, &opts);

    let (status, report) = server
        .import(
            "resume=true&map_path=C%3A%5CManga%3D%2Fdata%2Fmanga",
            zip_dir(userdata.dir(), ""),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report, serde_json::to_value(&cli).unwrap());
    assert_eq!(report["dry_run"], false);
    let tasks = server.db.tasks().list().unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().any(|t| t.save_to == "/data/manga/Guts"));
}

/// A zip of `userdata`'s files plus an entry named `evil`.
fn zip_with_entry(userdata: &Path, evil: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    zip.start_file(evil, options).unwrap();
    zip.write_all(b"owned").unwrap();
    for entry in std::fs::read_dir(userdata).unwrap() {
        let entry = entry.unwrap();
        zip.start_file(entry.file_name().to_str().unwrap(), options)
            .unwrap();
        zip.write_all(&std::fs::read(entry.path()).unwrap())
            .unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[tokio::test]
async fn an_entry_outside_the_folder_refuses_the_upload() {
    for evil in [
        "../evil.txt",
        "userdata/../../evil.txt",
        "/tmp/fmd2r-zip-slip-evil.txt",
    ] {
        let server = Server::new();
        let userdata = fmd2::every_source();

        let (status, problem) = server
            .import("", zip_with_entry(userdata.dir(), evil))
            .await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{evil}: {problem}");
        assert!(
            problem["detail"].as_str().unwrap().contains("outside"),
            "{problem}"
        );
        assert!(server.db.tasks().list().unwrap().is_empty(), "{evil}");
        assert!(!server._dir.path().join("evil.txt").exists());
        assert!(
            !server
                ._dir
                .path()
                .parent()
                .unwrap()
                .join("evil.txt")
                .exists()
        );
        assert!(!Path::new("/tmp/fmd2r-zip-slip-evil.txt").exists());
    }
}

#[tokio::test]
async fn an_upload_over_the_size_limit_is_refused() {
    let userdata = fmd2::every_source();
    let zip = zip_dir(userdata.dir(), "");
    let server = Server::with(|state| {
        state.with_import_limits(ImportLimits {
            upload_bytes: zip.len() as u64 - 1,
            ..ImportLimits::default()
        })
    });

    let (status, problem) = server.import("", zip).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{problem}");
    assert!(server.db.tasks().list().unwrap().is_empty());
    assert_eq!(server.leftovers(), Vec::<String>::new());
}

#[tokio::test]
async fn a_zip_that_unpacks_over_the_size_limit_is_refused() {
    let userdata = fmd2::every_source();
    let zip = zip_dir(userdata.dir(), "");
    let unpacked: u64 = std::fs::read_dir(userdata.dir())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    let server = Server::with(|state| {
        state.with_import_limits(ImportLimits {
            upload_bytes: zip.len() as u64,
            extracted_bytes: unpacked - 1,
        })
    });

    let (status, problem) = server.import("", zip).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{problem}");
    assert!(server.db.tasks().list().unwrap().is_empty());
    assert_eq!(server.leftovers(), Vec::<String>::new());
}

#[tokio::test]
async fn fmd2_timestamps_are_read_as_local_time_in_the_given_zone() {
    let server = Server::new();
    let userdata = fmd2::Fmd2::new();
    // FMD2 writes `Now`, the local wall-clock time, without a zone (baseunits/SQLiteData.pas:159-167).
    userdata.downloads(&[fmd2::DownloadRow {
        dateadded: "2024-03-05 14:07:09.123",
        datelastdownloaded: "2024-07-01 12:00:00.000",
        ..fmd2::DownloadRow::default()
    }]);
    userdata.favorites(&[fmd2::FavoriteRow {
        dateadded: "2023-01-02 03:04:05.006",
        ..fmd2::FavoriteRow::default()
    }]);

    let (status, report) = server
        .import("timezone=Europe%2FAmsterdam", zip_dir(userdata.dir(), ""))
        .await;

    assert_eq!(status, StatusCode::OK, "{report}");
    let task = &server.db.tasks().list().unwrap()[0];
    // 2024-03-05T13:07:09.123Z: CET is UTC+1.
    assert_eq!(task.date_added, 1_709_644_029_123);
    // 2024-07-01T10:00:00Z: CEST is UTC+2.
    assert_eq!(task.date_last_downloaded, Some(1_719_828_000_000));
    // 2023-01-02T02:04:05.006Z.
    assert_eq!(
        server.db.favorites().list().unwrap()[0].date_added,
        1_672_625_045_006
    );
}

#[tokio::test]
async fn an_unknown_time_zone_is_refused() {
    let server = Server::new();
    let userdata = fmd2::every_source();

    let (status, problem) = server
        .import("timezone=Mars%2FOlympus_Mons", zip_dir(userdata.dir(), ""))
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["field"], "timezone");
    assert!(server.db.tasks().list().unwrap().is_empty());
}

#[tokio::test]
async fn imported_settings_are_live_in_the_running_server() {
    let server = Server::new();
    let userdata = fmd2::every_source();

    let (status, report) = server.import("", zip_dir(userdata.dir(), "")).await;

    assert_eq!(status, StatusCode::OK, "{report}");
    // settings.json's connections/NumberOfTasks = 3.
    let settings = server.get("/api/settings").await;
    assert_eq!(
        settings["connections"]["max_parallel_tasks"], 3,
        "{settings}"
    );
}

#[tokio::test]
async fn an_import_runs_as_the_import_job() {
    let server = Server::new();
    let userdata = fmd2::every_source();

    let (status, _) = server
        .import("dry_run=true", zip_dir(userdata.dir(), ""))
        .await;

    assert_eq!(status, StatusCode::OK);
    let job = server.get("/api/jobs/import").await;
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(job["done"], job["total"], "{job}");
    assert_ne!(job["total"], 0, "{job}");
    assert_eq!(job["last_error"], Value::Null, "{job}");
}

#[tokio::test]
async fn a_refused_upload_fails_the_import_job() {
    let server = Server::new();
    let userdata = fmd2::every_source();

    let (status, _) = server
        .import("", zip_with_entry(userdata.dir(), "../evil.txt"))
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let job = server.get("/api/jobs/import").await;
    assert_eq!(job["state"], "failed", "{job}");
    assert!(
        job["last_error"].as_str().unwrap().contains("outside"),
        "{job}"
    );
}

#[tokio::test]
async fn only_one_import_runs_at_a_time() {
    let server = Server::new();
    let userdata = fmd2::every_source();
    let zip = zip_dir(userdata.dir(), "");
    // An upload that is still arriving.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    let first = tokio::spawn(
        build_router(server.state.clone()).oneshot(
            Request::post("/api/import")
                .header("content-type", "application/zip")
                .body(Body::from_stream(
                    tokio_stream::wrappers::ReceiverStream::new(rx),
                ))
                .unwrap(),
        ),
    );
    tx.send(Ok(zip[..10].to_vec())).await.unwrap();
    // Wait until the first import has started.
    server.wait_for_import_job().await;

    let (status, problem) = server.import("", zip.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");

    tx.send(Ok(zip[10..].to_vec())).await.unwrap();
    drop(tx);
    let first = first.await.unwrap().unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let (status, _) = server.import("", zip).await;
    assert_eq!(status, StatusCode::OK, "a finished import frees the job");
}

#[tokio::test]
async fn an_abandoned_upload_frees_the_import_job() {
    let server = Server::new();
    let userdata = fmd2::every_source();
    let zip = zip_dir(userdata.dir(), "");
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    let first = tokio::spawn(
        build_router(server.state.clone()).oneshot(
            Request::post("/api/import")
                .body(Body::from_stream(
                    tokio_stream::wrappers::ReceiverStream::new(rx),
                ))
                .unwrap(),
        ),
    );
    tx.send(Ok(zip[..10].to_vec())).await.unwrap();
    server.wait_for_import_job().await;

    // The client goes away mid-upload.
    first.abort();
    let _ = first.await;

    let (status, problem) = server.import("", zip).await;
    assert_eq!(status, StatusCode::OK, "{problem}");
}

/// An empty queue that counts how often it is asked to start waiting tasks.
#[derive(Clone, Default)]
struct CountingEngine(Arc<AtomicUsize>);

fn ready<T: Send + 'static>(value: T) -> BoxFuture<'static, T> {
    Box::pin(std::future::ready(value))
}

impl DownloadEngine for CountingEngine {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TaskInfo>, EngineError>> {
        ready(Ok(Vec::new()))
    }
    fn add(&self, download: NewDownload) -> BoxFuture<'_, Result<TaskId, EngineError>> {
        ready(Err(EngineError::NoModule(download.module_id)))
    }
    fn start(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }
    fn stop(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }
    fn redownload(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }
    fn enable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }
    fn disable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }
    fn start_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Ok(()))
    }
    fn stop_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Ok(()))
    }
    fn delete(&self, id: TaskId, _files: bool) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }
    fn reorder(&self, _ids: Vec<TaskId>) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Ok(()))
    }
    fn activate_waiting(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        ready(Ok(()))
    }
}

#[tokio::test]
async fn imported_waiting_tasks_start_without_a_restart() {
    let engine = CountingEngine::default();
    let server = Server::with(|state| state.with_engine(engine.clone()));
    let userdata = fmd2::every_source();

    let (status, _) = server
        .import("dry_run=true&resume=true", zip_dir(userdata.dir(), ""))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        engine.0.load(Ordering::SeqCst),
        0,
        "a dry run queues nothing"
    );

    let (status, _) = server
        .import("resume=true", zip_dir(userdata.dir(), ""))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(engine.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unreadable_fmd2_file_says_which_and_why() {
    let server = Server::new();
    let userdata = fmd2::Fmd2::new();
    userdata.file("modules.json", "{ not json");

    let (status, problem) = server.import("", zip_dir(userdata.dir(), "")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    let detail = problem["detail"].as_str().unwrap();
    assert!(detail.starts_with("modules.json: "), "{detail}");
}

#[tokio::test]
async fn an_import_the_client_stops_waiting_for_still_finishes() {
    let engine = CountingEngine::default();
    let server = Server::with(|state| state.with_engine(engine.clone()));
    let userdata = fmd2::every_source();
    let zip = zip_dir(userdata.dir(), "");
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    let request = tokio::spawn(
        build_router(server.state.clone()).oneshot(
            Request::post("/api/import")
                .body(Body::from_stream(
                    tokio_stream::wrappers::ReceiverStream::new(rx),
                ))
                .unwrap(),
        ),
    );
    // The whole upload arrives, then the client goes away while the import runs.
    tx.send(Ok(zip)).await.unwrap();
    drop(tx);
    server.wait_for_import_job().await;
    loop {
        let job = server.get("/api/jobs/import").await;
        if job["total"] != 0 || job["state"] != "running" {
            break;
        }
        tokio::task::yield_now().await;
    }
    request.abort();
    let _ = request.await;

    loop {
        let job = server.get("/api/jobs/import").await;
        if job["state"] != "running" {
            assert_eq!(job["state"], "done", "{job}");
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(server.db.tasks().list().unwrap().len(), 2);
    assert_eq!(engine.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_upload_declared_over_the_size_limit_is_refused_before_it_is_read() {
    let server = Server::with(|state| {
        state.with_import_limits(ImportLimits {
            upload_bytes: 1024,
            ..ImportLimits::default()
        })
    });

    let response = build_router(server.state.clone())
        .oneshot(
            Request::post("/api/import")
                .header("content-length", "1025")
                .body(Body::from("PK"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(server.leftovers(), Vec::<String>::new());
}
