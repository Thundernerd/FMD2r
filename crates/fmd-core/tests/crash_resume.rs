//! A download killed with SIGKILL resumes when the engine is opened again
//! (docs/tickets/T44-download-hard-crash-resume.md).
//!
//! The engine runs in a child process (this test binary with [`CHILD_DIR`] set, see [`child`]),
//! which the parent kills and restarts. Expected values: `CheckAndActiveTaskAtStartup` and
//! `CheckForExists` (baseunits/uDownloadsManager.pas:1003-1064, :1859-1893) reuse the saved page
//! list and skip pages on disk.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]
#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fmd_core::download::{ChapterSpec, DownloadManager, EngineConfig, NewDownload};
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_store::{AppDb, TaskStatus};
use serde_json::json;

/// Set in a child process: the directory it downloads in.
const CHILD_DIR: &str = "FMD2R_CRASH_CHILD_DIR";
/// Set in a child process: the image URLs holding this are never answered.
const CHILD_HOLD: &str = "FMD2R_CRASH_CHILD_HOLD";

/// How long a test waits for a child before failing.
const PATIENCE: Duration = Duration::from_secs(60);

/// The image every page is: a PNG signature and enough bytes that writing it takes a while,
/// so a kill can land while it is being written.
fn image() -> Vec<u8> {
    let mut image = b"\x89PNG\r\n\x1a\n".to_vec();
    image.extend((0..32u32 << 20).map(|i| (i % 251) as u8));
    image
}

/// A module whose chapter `/c/<n>` has `n` pages at `https://t/img/c/<n>/<page>`; it looks the
/// chapter up at `https://t/chapter/c/<n>` first, so the requests log shows each
/// `OnGetPageNumber`.
const T: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='t'; m.Name='T'; m.RootURL='https://t'; m.OnGetPageNumber='GPN' end
function GPN()
  HTTP.GET('https://t/chapter' .. URL)
  for i = 1, tonumber(URL:match('%d+$')) do TASK.PageLinks.Add('https://t/img' .. URL .. '/' .. i) end
  return true
end
"#;

/// Serves [`image`] for every URL holding `/img/`, a 404 otherwise, and appends every URL to
/// a log file that outlives the process.
struct StubTransport {
    image: Arc<Vec<u8>>,
    log: PathBuf,
    hold: Option<String>,
}

impl Transport for StubTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .unwrap();
        writeln!(log, "{}", request.url).unwrap();
        if self.hold.as_ref().is_some_and(|h| request.url.contains(h)) {
            return Box::pin(std::future::pending());
        }
        let image = request.url.contains("/img/");
        let response = WireResponse {
            status: if image { 200 } else { 404 },
            reason: String::new(),
            headers: Vec::new(),
            body: if image {
                self.image.to_vec()
            } else {
                Vec::new()
            },
        };
        Box::pin(async move { Ok(response) })
    }
}

/// The child process: downloads chapter `/c/3` of module `t` as a CBZ in `$CHILD_DIR` until the
/// task ends. Does nothing in a normal test run.
#[tokio::test(flavor = "multi_thread")]
async fn child() {
    let Some(dir) = std::env::var_os(CHILD_DIR).map(PathBuf::from) else {
        return;
    };
    let lua = dir.join("lua");
    fs::create_dir_all(lua.join("modules")).unwrap();
    fs::write(lua.join("modules/T.lua"), T).unwrap();
    let report = ModuleRegistry::load_dir(&lua);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let transport = Arc::new(StubTransport {
        image: Arc::new(image()),
        log: dir.join("requests.log"),
        hold: std::env::var(CHILD_HOLD).ok(),
    });
    let http = HttpClient::with_transport(transport).unwrap();
    let mut config = PoolConfig::new(http.clone());
    config.threads = 2;
    config.lua_dir = lua;
    let db = AppDb::open(dir.join("app.db")).unwrap();
    let settings = SettingsService::load(db.clone()).unwrap();
    settings
        .update(json!({
            "output": {"format": "cbz"},
            "connections": {"threads_per_task": 1, "auto_retry_failed_tasks": 0},
        }))
        .unwrap();
    let config = EngineConfig::new(
        db.clone(),
        Arc::new(WorkerPool::new(config).unwrap()),
        Arc::new(report.registry),
        Arc::new(settings),
        http,
    );
    let manager = DownloadManager::open(config).await.unwrap();
    if db.tasks().list().unwrap().is_empty() {
        manager
            .add_task(NewDownload {
                module_id: "t".into(),
                manga_link: "/manga".into(),
                title: "Manga".into(),
                chapters: vec![ChapterSpec {
                    link: "/c/3".into(),
                    title: "One".into(),
                    number: 1,
                }],
                save_to: dir.join("out").to_string_lossy().into_owned(),
                ..NewDownload::default()
            })
            .await
            .unwrap();
    }
    // The parent may open `app.db` from now on: its schema is in place.
    fs::write(dir.join("ready"), "").unwrap();
    loop {
        let status = db.tasks().list().unwrap()[0].status;
        if matches!(status, TaskStatus::Finished | TaskStatus::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    manager.shutdown().await.unwrap();
}

/// This test binary, running [`child`] in `dir`.
fn spawn_child(dir: &Path, hold: Option<&str>) -> Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["child", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_DIR, dir);
    match hold {
        Some(hold) => command.env(CHILD_HOLD, hold),
        None => command.env_remove(CHILD_HOLD),
    };
    command.spawn().unwrap()
}

/// Waits until `check` holds, polling as fast as it can, while `child` runs.
fn wait_until(child: &mut Child, what: &str, mut check: impl FnMut() -> bool) {
    let started = Instant::now();
    while !check() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the child exited ({status}) before {what}");
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            panic!("timed out waiting for {what}");
        }
        std::thread::yield_now();
    }
}

fn kill(mut child: Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}

/// Runs a child in `dir` until the task ends.
fn run_to_end(dir: &Path) {
    let mut child = spawn_child(dir, None);
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "the child failed: {status}");
            return;
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            panic!("the resumed task did not end");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How many times `url` was requested, over every child run in `dir`.
fn requests(dir: &Path, url: &str) -> usize {
    fs::read_to_string(dir.join("requests.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| *l == url)
        .count()
}

/// The status of the only task in `dir`'s `app.db`, `None` before it is queued.
fn status(dir: &Path) -> Option<TaskStatus> {
    let db = AppDb::open(dir.join("app.db")).unwrap();
    let tasks = db.tasks().list().unwrap();
    tasks.first().map(|t| t.status)
}

fn assert_finished_with_whole_pages(dir: &Path) {
    assert_eq!(status(dir), Some(TaskStatus::Finished));
    let archive = dir.join("out/Manga/One.cbz");
    let mut zip = zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_owned())
        .collect();
    assert_eq!(names, ["001.png", "002.png", "003.png"]);
    let image = image();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        assert!(data == image, "{} is not the whole image", entry.name());
    }
    // OnGetPageNumber ran once: the page list saved before the kill was used again.
    assert_eq!(requests(dir, "https://t/chapter/c/3"), 1);
}

/// The names in `dir`, empty when it does not exist.
fn names(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_task_killed_while_a_page_is_written_resumes_without_keeping_the_partial_page() {
    let dir = tempfile::tempdir().unwrap();
    let chapter = dir.path().join("out/Manga/One");
    // Page 3 never comes, so the task is still downloading when it is killed.
    let mut child = spawn_child(dir.path(), Some("/img/c/3/3"));
    wait_until(&mut child, "page 2 being written", || {
        names(&chapter).iter().any(|n| n.starts_with("002"))
    });
    kill(child);
    assert_eq!(status(dir.path()), Some(TaskStatus::Downloading));

    run_to_end(dir.path());

    assert_finished_with_whole_pages(dir.path());
    // Page 1 was on disk when the task resumed.
    assert_eq!(requests(dir.path(), "https://t/img/c/3/1"), 1);
    assert_eq!(names(&dir.path().join("out/Manga")), ["One.cbz"]);
}

#[test]
fn a_task_killed_while_packing_resumes_and_packs_every_page() {
    // From right at Compressing to well into writing the archive.
    for delay in [0, 5, 30, 80] {
        let dir = tempfile::tempdir().unwrap();
        let mut child = spawn_child(dir.path(), None);
        let mut db = None;
        wait_until(&mut child, "packing", || {
            // Opening `app.db` while the child migrates it races its migrations.
            if !dir.path().join("ready").exists() {
                return false;
            }
            let db = db.get_or_insert_with(|| AppDb::open(dir.path().join("app.db")).unwrap());
            let tasks = db.tasks().list().unwrap();
            tasks.first().map(|t| t.status) == Some(TaskStatus::Compressing)
        });
        std::thread::sleep(Duration::from_millis(delay));
        kill(child);

        run_to_end(dir.path());

        assert_finished_with_whole_pages(dir.path());
        // The pages were on disk, maybe half packed, when the task resumed.
        for page in 1..=3 {
            let url = format!("https://t/img/c/3/{page}");
            assert_eq!(requests(dir.path(), &url), 1, "{url} after {delay} ms");
        }
        // No half-written archive or page and no staged page is left behind.
        let manga = names(&dir.path().join("out/Manga"));
        assert_eq!(manga, ["One.cbz"], "after {delay} ms");
    }
}
