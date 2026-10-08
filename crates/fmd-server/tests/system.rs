//! System page endpoints (`/api/jobs`, `/api/logs`, `/api/about`) driven through `build_router`
//! with `oneshot`, a fake job registry and faked tool probes.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use fmd_server::{
    AppState, EventBus, LoadFailure, LogBuffer, ModuleCatalog, ModulesReport, ToolCheck, ToolProbe,
    build_router,
};
use fmd_store::AppDb;
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

struct Harness {
    dir: TempDir,
    state: AppState,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    Harness {
        dir,
        state: AppState::new(db).unwrap(),
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

async fn body_json(res: Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// A job that records what it was asked to do and reports a fixed status.
struct FakeJob {
    id: &'static str,
    title: &'static str,
    status: Mutex<JobStatus>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl FakeJob {
    fn new(id: &'static str, title: &'static str, status: JobStatus) -> Self {
        Self {
            id,
            title,
            status: Mutex::new(status),
            calls: Arc::default(),
        }
    }
}

impl Job for FakeJob {
    fn id(&self) -> &str {
        self.id
    }

    fn title(&self) -> &str {
        self.title
    }

    fn status(&self) -> JobStatus {
        self.status.lock().unwrap().clone()
    }

    fn run(&self) -> Result<(), JobError> {
        self.calls.lock().unwrap().push(format!("run {}", self.id));
        let mut status = self.status.lock().unwrap();
        if status.phase == JobPhase::Running {
            return Err(JobError::AlreadyRunning);
        }
        status.phase = JobPhase::Running;
        Ok(())
    }

    fn cancel(&self) -> Result<(), JobError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("cancel {}", self.id));
        let mut status = self.status.lock().unwrap();
        if status.phase != JobPhase::Running {
            return Err(JobError::NotRunning);
        }
        status.phase = JobPhase::Idle;
        Ok(())
    }
}

fn idle_favorites() -> FakeJob {
    FakeJob::new(
        "favorites",
        "Check favorites",
        JobStatus {
            phase: JobPhase::Idle,
            done: 0,
            total: 0,
            last_run: Some(1_759_910_400_000),
            next_run: Some(1_759_914_000_000),
            last_error: None,
        },
    )
}

fn failed_modules() -> FakeJob {
    FakeJob::new(
        "modules",
        "Update modules",
        JobStatus {
            phase: JobPhase::Failed,
            done: 3,
            total: 10,
            last_run: Some(1_759_910_400_000),
            next_run: None,
            last_error: Some("GitHub API: HTTP 403".into()),
        },
    )
}

#[tokio::test]
async fn jobs_lists_every_registered_job_with_its_status() {
    let h = harness();
    let jobs = JobRegistry::new();
    jobs.register(idle_favorites());
    jobs.register(failed_modules());
    let state = h.state.clone().with_jobs(jobs);

    let res = send(&state, get("/api/jobs")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(
        body,
        serde_json::json!([
            {
                "id": "favorites",
                "title": "Check favorites",
                "state": "idle",
                "done": 0,
                "total": 0,
                "last_run": "2025-10-08T08:00:00Z",
                "next_run": "2025-10-08T09:00:00Z",
                "last_error": null,
            },
            {
                "id": "modules",
                "title": "Update modules",
                "state": "failed",
                "done": 3,
                "total": 10,
                "last_run": "2025-10-08T08:00:00Z",
                "next_run": null,
                "last_error": "GitHub API: HTTP 403",
            },
        ])
    );
}

#[tokio::test]
async fn running_a_job_calls_it_and_answers_with_its_new_state() {
    let h = harness();
    let jobs = JobRegistry::new();
    let favorites = idle_favorites();
    let calls = favorites.calls.clone();
    jobs.register(favorites);
    jobs.register(failed_modules());
    let state = h.state.clone().with_jobs(jobs);

    let res = send(&state, post("/api/jobs/favorites/run")).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(body_json(res).await["state"], "running");
    assert_eq!(*calls.lock().unwrap(), ["run favorites"]);

    let res = send(&state, post("/api/jobs/favorites/run")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(res.headers()["content-type"], "application/problem+json");

    let res = send(&state, post("/api/jobs/favorites/cancel")).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(body_json(res).await["state"], "idle");
    assert_eq!(
        *calls.lock().unwrap(),
        ["run favorites", "run favorites", "cancel favorites"]
    );
}

#[tokio::test]
async fn one_job_is_read_by_id() {
    let h = harness();
    let jobs = JobRegistry::new();
    jobs.register(idle_favorites());
    jobs.register(failed_modules());
    let state = h.state.clone().with_jobs(jobs);

    let res = send(&state, get("/api/jobs/modules")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let job = body_json(res).await;
    assert_eq!(job["id"], "modules");
    assert_eq!(job["state"], "failed");
    assert_eq!(job["last_error"], "GitHub API: HTTP 403");

    let res = send(&state, get("/api/jobs/x")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn updating_modules_runs_the_modules_job() {
    let h = harness();
    let jobs = JobRegistry::new();
    let modules = failed_modules();
    let calls = modules.calls.clone();
    jobs.register(modules);
    let state = h.state.clone().with_jobs(jobs);

    let res = send(&state, post("/api/modules/update")).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(body_json(res).await["state"], "running");
    assert_eq!(*calls.lock().unwrap(), ["run modules"]);

    let res = send(&h.state, post("/api/modules/update")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancelling_a_job_that_is_not_running_is_a_409() {
    let h = harness();
    let jobs = JobRegistry::new();
    jobs.register(idle_favorites());
    let state = h.state.clone().with_jobs(jobs);

    let res = send(&state, post("/api/jobs/favorites/cancel")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn controlling_an_unknown_job_is_a_404() {
    let h = harness();
    let jobs = JobRegistry::new();
    jobs.register(idle_favorites());
    let state = h.state.clone().with_jobs(jobs);

    for uri in ["/api/jobs/x/run", "/api/jobs/x/cancel"] {
        let res = send(&state, post(uri)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body_json(res).await["status"], 404);
    }
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

#[tokio::test]
async fn job_changes_stream_as_job_state_events() {
    let h = harness();
    let jobs = JobRegistry::new();
    jobs.register(failed_modules());
    let state = h.state.clone().with_jobs(jobs.clone());
    let res = send(&state, get("/api/events")).await;

    jobs.changed("modules");
    let seen = read_sse_until(res, "GitHub API").await;
    assert!(seen.contains("event: job.state"), "{seen}");
    assert!(seen.contains(r#""id":"modules""#), "{seen}");
    assert!(seen.contains(r#""done":3"#), "{seen}");
}

fn emit_logs(logs: &LogBuffer, emit: impl FnOnce()) {
    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry().with(logs.clone());
    tracing::subscriber::with_default(subscriber, emit);
}

/// Lines as Lua modules (`fmd.logger`, baseunits/lua/LuaLogger.pas:15-46) and the server log them.
fn mixed_logs() -> LogBuffer {
    let logs = LogBuffer::new(100, EventBus::new());
    emit_logs(&logs, || {
        tracing::info!(target: "fmd.logger", module = "MangaDex", "chapter list loaded");
        tracing::warn!(target: "fmd.logger", module = "MangaDex", "rate limited");
        tracing::error!(target: "fmd.logger", module = "Bato.to", "no pages");
        tracing::debug!(target: "fmd_server", "request served");
        tracing::warn!(target: "fmd_server", "slow request");
    });
    logs
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
async fn logs_filter_by_minimum_level() {
    let h = harness();
    let state = h.state.clone().with_logs(mixed_logs());

    let lines = body_json(send(&state, get("/api/logs?level=warn")).await).await;
    assert_eq!(
        messages(&lines),
        ["rate limited", "no pages", "slow request"]
    );
    let lines = body_json(send(&state, get("/api/logs?level=error")).await).await;
    assert_eq!(messages(&lines), ["no pages"]);
}

#[tokio::test]
async fn logs_filter_by_website_module_and_carry_it() {
    let h = harness();
    let state = h.state.clone().with_logs(mixed_logs());

    let lines = body_json(send(&state, get("/api/logs?module=MangaDex")).await).await;
    assert_eq!(messages(&lines), ["chapter list loaded", "rate limited"]);
    assert_eq!(lines[0]["module"], "MangaDex");
    assert_eq!(lines[0]["target"], "fmd.logger");

    let all = body_json(send(&state, get("/api/logs")).await).await;
    assert_eq!(all[3]["module"], serde_json::Value::Null);
}

#[tokio::test]
async fn logs_limit_keeps_the_newest_matching_lines() {
    let h = harness();
    let state = h.state.clone().with_logs(mixed_logs());

    let lines = body_json(send(&state, get("/api/logs?level=warn&limit=2")).await).await;
    assert_eq!(messages(&lines), ["no pages", "slow request"]);
}

#[tokio::test]
async fn logs_since_a_sequence_number_page_forward_from_it() {
    let h = harness();
    let logs = mixed_logs();
    let state = h.state.clone().with_logs(logs.clone());
    let first = logs.since(None)[0].seq;

    // Catching up after line 1, two at a time, must not skip any line.
    let page =
        body_json(send(&state, get(&format!("/api/logs?since={first}&limit=2"))).await).await;
    assert_eq!(messages(&page), ["rate limited", "no pages"]);
}

#[tokio::test]
async fn an_unknown_log_level_is_a_400() {
    let h = harness();
    let res = send(&h.state, get("/api/logs?level=loud")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// Tool probes with fixed answers: python3 and node present, magick missing, FlareSolverr down.
struct FakeTools;

impl ToolProbe for FakeTools {
    fn probe(&self) -> Vec<ToolCheck> {
        let check = |name: &str, ok: bool, detail: &str| ToolCheck {
            name: name.into(),
            ok,
            detail: detail.into(),
        };
        vec![
            check("python3", true, "Python 3.12.3"),
            check("node", true, "v22.4.0"),
            check("magick", false, "not found on PATH"),
            check("FlareSolverr", false, "localhost:8191: connection refused"),
        ]
    }
}

struct FakeModules;

impl ModuleCatalog for FakeModules {
    fn report(&self) -> ModulesReport {
        ModulesReport {
            upstream_ref: Some("master".into()),
            upstream_sha: Some("4f2c9e1".into()),
            module_count: 665,
            xpath_backend: Some("fpc".into()),
            load_failures: vec![LoadFailure {
                module: "Batoto.lua".into(),
                error: "attempt to call a nil value (field 'ResolveRedirect')".into(),
                inbox_id: Some("12".into()),
            }],
        }
    }
}

#[tokio::test]
async fn about_reports_version_modules_and_tool_checks() {
    let h = harness();
    let state = h
        .state
        .clone()
        .with_tools(FakeTools)
        .with_modules(FakeModules);

    let res = send(&state, get("/api/about")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let about = body_json(res).await;
    assert_eq!(about["version"], env!("CARGO_PKG_VERSION"));
    assert!(about["git_revision"].is_string() || about["git_revision"].is_null());
    assert_eq!(about["upstream_ref"], "master");
    assert_eq!(about["upstream_sha"], "4f2c9e1");
    assert_eq!(about["module_count"], 665);
    assert_eq!(about["xpath_backend"], "fpc");
    assert_eq!(
        about["load_failures"],
        serde_json::json!([{
            "module": "Batoto.lua",
            "error": "attempt to call a nil value (field 'ResolveRedirect')",
            "inbox_id": "12",
        }])
    );
    assert!(about["uptime_secs"].is_u64());
    assert_eq!(
        about["tools"],
        serde_json::json!([
            { "name": "python3", "ok": true, "detail": "Python 3.12.3" },
            { "name": "node", "ok": true, "detail": "v22.4.0" },
            { "name": "magick", "ok": false, "detail": "not found on PATH" },
            { "name": "FlareSolverr", "ok": false, "detail": "localhost:8191: connection refused" },
        ])
    );
}

#[tokio::test]
async fn about_lists_the_data_dir_and_database_sizes() {
    let h = harness();
    let state = h
        .state
        .clone()
        .with_tools(FakeTools)
        .with_data_dir(h.dir.path());

    let about = body_json(send(&state, get("/api/about")).await).await;
    assert_eq!(about["data_dir"], h.dir.path().to_str().unwrap());
    let dbs = about["databases"].as_array().unwrap();
    assert_eq!(dbs[0]["name"], "app.db");
    assert!(dbs[0]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(dbs[1]["name"], "lists.db");
    assert!(dbs[1]["bytes"].is_null(), "lists.db was never created");
}
