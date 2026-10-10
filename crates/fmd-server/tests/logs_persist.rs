//! Persisted logs: `GET /api/logs` across a simulated restart, and `LogWriter` rotation.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_server::{
    AppState, EventBus, LogBuffer, LogLevel, LogLine, LogRotation, LogWriter, build_router,
};
use fmd_store::AppDb;
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

const ROTATION: LogRotation = LogRotation {
    max_file_bytes: 10 * 1024 * 1024,
    max_files: 5,
};

/// One server run: a fresh buffer persisted to `logs_dir` behind a fresh state.
struct Run {
    _db_dir: TempDir,
    logs: LogBuffer,
    state: AppState,
}

fn start(logs_dir: &Path) -> Run {
    start_with(logs_dir, 100, ROTATION)
}

fn start_with(logs_dir: &Path, capacity: usize, rotation: LogRotation) -> Run {
    let db_dir = tempfile::tempdir().unwrap();
    let logs = LogBuffer::new(capacity, EventBus::new());
    logs.persist(logs_dir, rotation).unwrap();
    let db = AppDb::open(db_dir.path().join("app.db")).unwrap();
    let state = AppState::new(db).unwrap().with_logs(logs.clone());
    Run {
        _db_dir: db_dir,
        logs,
        state,
    }
}

fn emit_logs(logs: &LogBuffer, emit: impl FnOnce()) {
    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry().with(logs.clone());
    tracing::subscriber::with_default(subscriber, emit);
}

async fn send(state: &AppState, uri: &str) -> Response {
    let req = Request::get(uri).body(Body::empty()).unwrap();
    build_router(state.clone()).oneshot(req).await.unwrap()
}

async fn get_json(state: &AppState, uri: &str) -> serde_json::Value {
    let res = send(state, uri).await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn messages(lines: &serde_json::Value) -> Vec<&str> {
    lines
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["message"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn lines_logged_before_a_restart_are_served_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let before = start(dir.path());
    emit_logs(&before.logs, || {
        tracing::info!(target: "fmd.logger", module = "MangaDex", "chapter list loaded");
        tracing::error!(target: "fmd_server", "download failed");
    });
    drop(before);

    let after = start(dir.path());
    let lines = get_json(&after.state, "/api/logs").await;
    assert_eq!(messages(&lines), ["chapter list loaded", "download failed"]);
    assert_eq!(lines[0]["module"], "MangaDex");
    assert_eq!(lines[1]["level"], "ERROR");
}

#[tokio::test]
async fn since_pages_forward_across_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let before = start(dir.path());
    emit_logs(&before.logs, || {
        tracing::info!("one");
        tracing::info!("two");
    });
    let seen = get_json(&before.state, "/api/logs").await;
    let last_seen = seen[1]["seq"].as_u64().unwrap();
    drop(before);

    let after = start(dir.path());
    emit_logs(&after.logs, || {
        tracing::info!("three");
        tracing::info!("four");
    });
    let next = get_json(&after.state, &format!("/api/logs?since={last_seen}")).await;
    assert_eq!(messages(&next), ["three", "four"]);
    let page = get_json(&after.state, "/api/logs?since=0&limit=3").await;
    assert_eq!(messages(&page), ["one", "two", "three"]);
}

#[tokio::test]
async fn the_tail_after_a_restart_spans_rotated_files() {
    let dir = tempfile::tempdir().unwrap();
    // About two lines per file, so ten lines span several files.
    let rotation = LogRotation {
        max_file_bytes: 300,
        max_files: 10,
    };
    let before = start_with(dir.path(), 100, rotation);
    emit_logs(&before.logs, || {
        for n in 1..=10 {
            tracing::info!("line {n}");
        }
    });
    drop(before);

    let after = start_with(dir.path(), 5, rotation);
    let lines = get_json(&after.state, "/api/logs").await;
    assert_eq!(
        messages(&lines),
        ["line 6", "line 7", "line 8", "line 9", "line 10"]
    );
}

#[tokio::test]
async fn a_line_cut_off_by_a_crash_keeps_its_sequence_number_retired() {
    let dir = tempfile::tempdir().unwrap();
    let before = start(dir.path());
    emit_logs(&before.logs, || {
        tracing::info!("one");
        tracing::info!("two");
    });
    drop(before);
    // The process died while writing line 3, after a client could have seen it.
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join("fmd2r.log"))
        .unwrap();
    std::io::Write::write_all(&mut file, br#"{"seq":3,"time":"2026-10-"#).unwrap();
    drop(file);

    let after = start(dir.path());
    emit_logs(&after.logs, || tracing::info!("three"));
    let next = get_json(&after.state, "/api/logs?since=3").await;
    assert_eq!(messages(&next), ["three"]);
    let all = get_json(&after.state, "/api/logs").await;
    assert_eq!(messages(&all), ["one", "two", "three"]);
}

#[tokio::test]
async fn download_returns_the_persisted_lines_from_before_and_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let before = start(dir.path());
    emit_logs(&before.logs, || tracing::info!("before"));
    drop(before);
    let after = start(dir.path());
    emit_logs(&after.logs, || tracing::warn!("after"));

    let res = send(&after.state, "/api/logs/download").await;
    assert_eq!(res.status(), StatusCode::OK);
    let disposition = res.headers()["content-disposition"].to_str().unwrap();
    assert!(disposition.starts_with("attachment"), "{disposition}");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let messages: Vec<&str> = lines
        .iter()
        .map(|l| l["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages, ["before", "after"]);
    assert_eq!(lines[1]["level"], "WARN");
}

#[tokio::test]
async fn download_without_persisted_logs_returns_the_buffered_lines() {
    let db_dir = tempfile::tempdir().unwrap();
    let logs = LogBuffer::new(100, EventBus::new());
    emit_logs(&logs, || tracing::info!("in memory"));
    let db = AppDb::open(db_dir.path().join("app.db")).unwrap();
    let state = AppState::new(db).unwrap().with_logs(logs);

    let res = send(&state, "/api/logs/download").await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let line: serde_json::Value =
        serde_json::from_str(std::str::from_utf8(&bytes).unwrap().trim_end()).unwrap();
    assert_eq!(line["message"], "in memory");
}

fn line(seq: u64) -> LogLine {
    LogLine {
        seq,
        time: "2026-10-09T12:00:00Z".into(),
        level: LogLevel::Info,
        target: "fmd_server".into(),
        module: None,
        message: format!("line {seq}"),
    }
}

/// Every file in `dir` with its size, by name.
fn files(dir: &Path) -> Vec<(String, u64)> {
    let mut files: Vec<(String, u64)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let name = e.file_name().into_string().unwrap();
            (name, e.metadata().unwrap().len())
        })
        .collect();
    files.sort();
    files
}

/// Lines written by `line` for seqs 1000..=9999 all serialize to this length.
fn line_len() -> u64 {
    serde_json::to_vec(&line(1000)).unwrap().len() as u64 + 1
}

#[test]
fn rotation_bounds_the_files_by_size_and_count() {
    let dir = tempfile::tempdir().unwrap();
    // Three lines fit a file.
    let rotation = LogRotation {
        max_file_bytes: 3 * line_len(),
        max_files: 3,
    };
    let mut writer = LogWriter::open(dir.path(), rotation).unwrap();
    for seq in 1001..=1100 {
        writer.write(&line(seq)).unwrap();
    }

    // 100 lines are 33 full files and one with line 100: only the newest three are kept.
    assert_eq!(
        files(dir.path()),
        [
            ("fmd2r.1.log".to_owned(), 3 * line_len()),
            ("fmd2r.2.log".to_owned(), 3 * line_len()),
            ("fmd2r.log".to_owned(), line_len()),
        ]
    );
    let current = std::fs::read_to_string(dir.path().join("fmd2r.log")).unwrap();
    assert!(current.contains("line 1100"), "{current}");
    let oldest = std::fs::read_to_string(dir.path().join("fmd2r.2.log")).unwrap();
    assert!(oldest.contains("line 1094"), "{oldest}");
}

#[test]
fn a_lower_file_count_removes_the_files_past_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut writer = LogWriter::open(
        dir.path(),
        LogRotation {
            max_file_bytes: line_len(),
            max_files: 5,
        },
    )
    .unwrap();
    for seq in 1001..=1010 {
        writer.write(&line(seq)).unwrap();
    }
    drop(writer);
    assert_eq!(files(dir.path()).len(), 5);

    LogWriter::open(
        dir.path(),
        LogRotation {
            max_file_bytes: line_len(),
            max_files: 2,
        },
    )
    .unwrap();
    let names: Vec<String> = files(dir.path()).into_iter().map(|f| f.0).collect();
    assert_eq!(names, ["fmd2r.1.log", "fmd2r.log"]);
}

#[test]
fn a_single_file_starts_over_when_full() {
    let dir = tempfile::tempdir().unwrap();
    let rotation = LogRotation {
        max_file_bytes: 2 * line_len(),
        max_files: 1,
    };
    let mut writer = LogWriter::open(dir.path(), rotation).unwrap();
    for seq in 1001..=1005 {
        writer.write(&line(seq)).unwrap();
    }
    assert_eq!(files(dir.path()), [("fmd2r.log".to_owned(), line_len())]);
}
