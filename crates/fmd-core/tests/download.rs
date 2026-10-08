//! The download engine through the public `DownloadManager` API, with a real `WorkerPool`
//! running fixture modules, a stub HTTP transport serving images, a temp `app.db` and a temp
//! output dir (docs/tickets/T20-download-engine.md, "Seams under test").
//!
//! Expected values come from FMD2's `TTaskThread.Execute` and `TDownloadThread.DownloadImage`
//! (baseunits/uDownloadsManager.pas:334-412, :975-1374) and its `TDownloadManager`
//! (baseunits/uDownloadsManager.pas:1784-1985).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fmd_core::download::{ChapterSpec, DownloadManager, EngineConfig, EngineEvent, NewDownload};
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_store::{AppDb, TaskId, TaskStatus};
use serde_json::{Value, json};
use tokio::sync::broadcast;

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// How long a test waits for the engine before failing.
const PATIENCE: Duration = Duration::from_secs(20);

/// Serves a PNG for every URL holding `/img/`, a 404 otherwise; can fail a URL a number of
/// times first, or never answer the URLs holding a pattern.
#[derive(Default)]
struct StubTransport {
    requests: Mutex<Vec<String>>,
    failures: Mutex<HashMap<String, u32>>,
    hold: Mutex<Vec<String>>,
}

impl StubTransport {
    fn fail(&self, url: &str, times: u32) {
        self.failures.lock().unwrap().insert(url.into(), times);
    }

    /// Never answers the URLs holding `pattern`; an empty pattern releases them all.
    fn hold(&self, pattern: &str) {
        let mut hold = self.hold.lock().unwrap();
        match pattern {
            "" => hold.clear(),
            pattern => hold.push(pattern.into()),
        }
    }

    fn count(&self, url: &str) -> usize {
        self.requests().iter().filter(|r| *r == url).count()
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Transport for StubTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.requests.lock().unwrap().push(request.url.clone());
        if let Some(left) = self.failures.lock().unwrap().get_mut(&request.url)
            && *left > 0
        {
            *left -= 1;
            return Box::pin(async { Err(TransportError("refused".into())) });
        }
        let held = self
            .hold
            .lock()
            .unwrap()
            .iter()
            .any(|p| request.url.contains(p));
        if held {
            return Box::pin(std::future::pending());
        }
        let image = request.url.contains("/img/");
        let response = WireResponse {
            status: if image { 200 } else { 404 },
            reason: String::new(),
            headers: Vec::new(),
            body: if image { PNG.to_vec() } else { Vec::new() },
        };
        Box::pin(async move { Ok(response) })
    }
}

/// A module whose chapter `/c/<n>` has `n` pages at `https://t/img/c/<n>/<page>`; it looks the
/// chapter up at `https://t/chapter/c/<n>` first.
const T: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='t'; m.Name='T'; m.RootURL='https://t'; m.OnGetPageNumber='GPN' end
function GPN()
  HTTP.GET('https://t/chapter' .. URL)
  for i = 1, tonumber(URL:match('%d+$')) do TASK.PageLinks.Add('https://t/img' .. URL .. '/' .. i) end
  return true
end
"#;

/// A lua dir with fixture modules, an `app.db`, an output dir and what the engine runs on.
struct Fixture {
    dir: tempfile::TempDir,
    db: AppDb,
    registry: Arc<ModuleRegistry>,
    pool: Arc<WorkerPool>,
    http: HttpClient,
    settings: Arc<SettingsService>,
    transport: Arc<StubTransport>,
}

impl Fixture {
    /// `settings` is a merge patch over the default settings.
    fn new(modules: &[(&str, &str)], settings: Value) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let lua = dir.path().join("lua");
        fs::create_dir_all(lua.join("modules")).unwrap();
        for (name, source) in modules {
            fs::write(lua.join("modules").join(name), source).unwrap();
        }
        let report = ModuleRegistry::load_dir(&lua);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let transport = Arc::new(StubTransport::default());
        let http = HttpClient::with_transport(transport.clone()).unwrap();
        let mut config = PoolConfig::new(http.clone());
        config.threads = 4;
        config.lua_dir = lua;
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let service = SettingsService::load(db.clone()).unwrap();
        service.update(settings).unwrap();
        Fixture {
            db,
            registry: Arc::new(report.registry),
            pool: Arc::new(WorkerPool::new(config).unwrap()),
            http,
            settings: Arc::new(service),
            transport,
            dir,
        }
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    async fn manager(&self) -> DownloadManager {
        let config = EngineConfig::new(
            self.db.clone(),
            self.pool.clone(),
            self.registry.clone(),
            self.settings.clone(),
            self.http.clone(),
        );
        DownloadManager::open(config).await.unwrap()
    }

    /// A download of `chapters` (link, title) of module `module`'s manga "Manga".
    fn download(&self, module: &str, chapters: &[(&str, &str)]) -> NewDownload {
        NewDownload {
            module_id: module.into(),
            manga_link: "/manga".into(),
            title: "Manga".into(),
            chapters: chapters
                .iter()
                .enumerate()
                .map(|(i, (link, title))| ChapterSpec {
                    link: (*link).into(),
                    title: (*title).into(),
                    number: i as u32 + 1,
                })
                .collect(),
            save_to: self.out().to_string_lossy().into_owned(),
            ..NewDownload::default()
        }
    }
}

/// The statuses task `id` goes through, up to the first of `until`.
async fn statuses(
    events: &mut broadcast::Receiver<EngineEvent>,
    id: TaskId,
    until: &[TaskStatus],
) -> Vec<TaskStatus> {
    let mut seen = Vec::new();
    let collect = async {
        loop {
            match events.recv().await.unwrap() {
                EngineEvent::Status { task, status, .. } if task == id => {
                    seen.push(status);
                    if until.contains(&status) {
                        return;
                    }
                }
                _ => {}
            }
        }
    };
    if tokio::time::timeout(PATIENCE, collect).await.is_err() {
        panic!("task {id:?} did not reach {until:?}; saw {seen:?}");
    }
    seen
}

/// The entries of the zip at `path`, in order.
fn entries(path: &Path) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_two_chapter_task_is_downloaded_packed_and_marked_downloaded() {
    let f = Fixture::new(&[("T.lua", T)], json!({"output": {"format": "cbz"}}));
    let manager = f.manager().await;
    let mut events = manager.subscribe();

    let id = manager
        .add_task(f.download("t", &[("/c/2", "One"), ("/c/3", "Two")]))
        .await
        .unwrap();

    // Each chapter goes Preparing, Downloading, Converting, Compressing
    // (baseunits/uDownloadsManager.pas:1194, :1268, :1280, :1289); the task ends Finished
    // (:1361).
    use TaskStatus::*;
    assert_eq!(
        statuses(&mut events, id, &[Finished, Failed]).await,
        [
            Waiting,
            Preparing,
            Downloading,
            Converting,
            Compressing,
            Preparing,
            Downloading,
            Converting,
            Compressing,
            Finished
        ]
    );
    let manga = f.out().join("Manga");
    assert_eq!(entries(&manga.join("One.cbz")), ["001.png", "002.png"]);
    assert_eq!(
        entries(&manga.join("Two.cbz")),
        ["001.png", "002.png", "003.png"]
    );
    assert!(!manga.join("One").exists());
    assert_eq!(
        f.db.downloaded_chapters().list_for("t", "/manga").unwrap(),
        ["/c/2", "/c/3"]
    );
}

/// A `DynamicPageLink` module: its chapters have 3 pages whose links `OnGetImageURL` finds by
/// fetching `https://d/page/<page>`.
const D: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='d'; m.Name='D'; m.RootURL='https://d'; m.DynamicPageLink=true; m.OnGetPageNumber='GPN'; m.OnGetImageURL='GIU' end
function GPN() TASK.PageNumber = 3; return true end
function GIU()
  HTTP.GET('https://d/page/' .. WORKID)
  TASK.PageLinks[WORKID] = 'https://d/img/' .. WORKID
  return true
end
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_dynamic_page_link_module_gets_each_image_url_right_before_its_download() {
    let f = Fixture::new(&[("D.lua", D)], json!({"output": {"format": "cbz"}}));
    let manager = f.manager().await;
    let mut events = manager.subscribe();

    let id = manager
        .add_task(f.download("d", &[("/c", "One")]))
        .await
        .unwrap();

    use TaskStatus::*;
    // No page-link phase: straight from Preparing to Downloading
    // (baseunits/uDownloadsManager.pas:1214).
    assert_eq!(
        statuses(&mut events, id, &[Finished, Failed]).await,
        [
            Waiting,
            Preparing,
            Downloading,
            Converting,
            Compressing,
            Finished
        ]
    );
    assert_eq!(
        f.transport.requests(),
        [
            "https://d/page/0",
            "https://d/img/0",
            "https://d/page/1",
            "https://d/img/1",
            "https://d/page/2",
            "https://d/img/2",
        ]
    );
    assert_eq!(
        entries(&f.out().join("Manga/One.cbz")),
        ["001.png", "002.png", "003.png"]
    );
}

/// A module that downloads each page from its container link itself and saves it itself.
const C: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='c'; m.Name='C'; m.RootURL='https://c'; m.OnGetPageNumber='GPN'; m.OnDownloadImage='DI'; m.OnSaveImage='SI' end
function GPN()
  for i = 1, 2 do TASK.PageLinks.Add('unused' .. i); TASK.PageContainerLinks.Add('https://c/img/raw' .. i) end
  return true
end
function DI() return HTTP.GET(URL .. '?custom') end
function SI()
  local file = PATH .. FILENAME .. '.png'
  local f = io.open(file, 'wb'); f:write('saved by SI ' .. HTTP.Document.Size); f:close()
  return file
end
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_module_downloading_and_saving_its_images_itself_is_used_instead_of_the_built_in_ones() {
    let f = Fixture::new(&[("C.lua", C)], json!({}));
    let manager = f.manager().await;
    let mut events = manager.subscribe();

    let id = manager
        .add_task(f.download("c", &[("/c", "One")]))
        .await
        .unwrap();

    let statuses = statuses(&mut events, id, &[TaskStatus::Finished, TaskStatus::Failed]).await;
    assert_eq!(statuses.last(), Some(&TaskStatus::Finished));
    // `OnDownloadImage` gets the container link when there is one per page
    // (baseunits/uDownloadsManager.pas:372-374).
    assert_eq!(
        f.transport.requests(),
        ["https://c/img/raw1?custom", "https://c/img/raw2?custom"]
    );
    // The default output format keeps the images in the chapter's folder.
    let saved = |name: &str| fs::read_to_string(f.out().join("Manga/One").join(name)).unwrap();
    let expected = format!("saved by SI {}", PNG.len());
    assert_eq!(saved("001.png"), expected);
    assert_eq!(saved("002.png"), expected);
}

/// The statuses of a one-chapter task of module `t` whose page 3 fails twice, with failed
/// chapters retried `retries` times.
async fn with_page_3_failing_twice(retries: u32) -> (Fixture, TaskId, Vec<TaskStatus>) {
    let settings = json!({
        "output": {"format": "cbz"},
        "connections": {"auto_retry_failed_tasks": retries},
    });
    let f = Fixture::new(&[("T.lua", T)], settings);
    f.transport.fail("https://t/img/c/3/3", 2);
    let manager = f.manager().await;
    let mut events = manager.subscribe();
    let id = manager
        .add_task(f.download("t", &[("/c/3", "One")]))
        .await
        .unwrap();
    let seen = statuses(&mut events, id, &[TaskStatus::Finished]).await;
    drop(manager);
    (f, id, seen)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_chapter_is_retried_until_its_pages_come_through() {
    // The task finishes once the chapter's last retry gets page 3
    // (baseunits/uDownloadsManager.pas:1326-1338).
    let (f, id, seen) = with_page_3_failing_twice(2).await;
    use TaskStatus::*;
    let failures = seen.iter().filter(|s| **s == Failed).count();
    assert_eq!(failures, 2, "{seen:?}");
    assert_eq!(seen.last(), Some(&Finished));
    assert_eq!(
        entries(&f.out().join("Manga/One.cbz")),
        ["001.png", "002.png", "003.png"]
    );
    let chapters = f.db.tasks().chapters(id).unwrap();
    assert_eq!(chapters[0].status, fmd_store::ChapterStatus::Downloaded);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_retries_a_chapter_missing_a_page_fails_the_task() {
    let settings = json!({
        "output": {"format": "cbz"},
        "connections": {"auto_retry_failed_tasks": 0},
    });
    let f = Fixture::new(&[("T.lua", T)], settings);
    f.transport.fail("https://t/img/c/3/3", 2);
    let manager = f.manager().await;
    let mut events = manager.subscribe();
    let id = manager
        .add_task(f.download("t", &[("/c/3", "One")]))
        .await
        .unwrap();

    use TaskStatus::*;
    let seen = statuses(&mut events, id, &[Finished, Failed]).await;
    assert_eq!(seen, [Waiting, Preparing, Downloading, Failed]);
    // The task thread ends after the chapter, with the task Failed (:1346-1355).
    tokio::time::sleep(Duration::from_millis(200)).await;
    let task = f.db.tasks().get(id).unwrap().unwrap();
    assert_eq!(task.status, Failed);
    let chapters = f.db.tasks().chapters(id).unwrap();
    assert_eq!(chapters[0].status, fmd_store::ChapterStatus::Failed);
    assert!(!f.out().join("Manga/One.cbz").exists());
    assert!(
        f.db.downloaded_chapters()
            .list_for("t", "/manga")
            .unwrap()
            .is_empty()
    );
}

/// Waits until `check` holds.
async fn eventually(what: &str, check: impl Fn() -> bool) {
    let wait = async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    if tokio::time::timeout(PATIENCE, wait).await.is_err() {
        panic!("timed out waiting for {what}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_cut_off_mid_download_resumes_from_the_same_db_skipping_saved_pages() {
    let f = Fixture::new(&[("T.lua", T)], json!({"output": {"format": "cbz"}}));
    f.transport.hold("/img/c/3/2");
    let manager = f.manager().await;
    let id = manager
        .add_task(f.download("t", &[("/c/3", "One")]))
        .await
        .unwrap();
    let page_1 = f.out().join("Manga/One/001.png");
    eventually("page 1", || page_1.exists()).await;
    eventually("page 2 requested", || {
        f.transport.count("https://t/img/c/3/2") == 1
    })
    .await;

    drop(manager);
    let task = f.db.tasks().get(id).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Downloading);

    f.transport.hold("");
    let manager = f.manager().await;
    let mut events = manager.subscribe();
    if f.db.tasks().get(id).unwrap().unwrap().status != TaskStatus::Finished {
        statuses(&mut events, id, &[TaskStatus::Finished, TaskStatus::Failed]).await;
    }

    let task = f.db.tasks().get(id).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Finished);
    assert_eq!(
        entries(&f.out().join("Manga/One.cbz")),
        ["001.png", "002.png", "003.png"]
    );
    // The page list was saved, so the module is not asked for it again, and page 1 was on
    // disk (baseunits/uDownloadsManager.pas:1180-1206).
    assert_eq!(f.transport.count("https://t/chapter/c/3"), 1);
    assert_eq!(f.transport.count("https://t/img/c/3/1"), 1);
    assert_eq!(f.transport.count("https://t/img/c/3/2"), 2);
}

/// Module `t` limited to one task at a time.
const ONE_TASK: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='t'; m.Name='T'; m.RootURL='https://t'; m.OnGetPageNumber='GPN'; m.MaxTaskLimit=1 end
function GPN()
  for i = 1, tonumber(URL:match('%d+$')) do TASK.PageLinks.Add('https://t/img' .. URL .. '/' .. i) end
  return true
end
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_second_task_of_a_module_limited_to_one_task_waits_for_the_first() {
    let settings = json!({"connections": {"max_parallel_tasks": 4}});
    let f = Fixture::new(&[("T.lua", ONE_TASK)], settings);
    f.transport.hold("/img/c/1/");
    let manager = f.manager().await;
    let mut events = manager.subscribe();
    let first = manager
        .add_task(f.download("t", &[("/c/1", "One")]))
        .await
        .unwrap();
    eventually("the first task's page", || {
        f.transport.count("https://t/img/c/1/1") == 1
    })
    .await;

    let second = manager
        .add_task(f.download("t", &[("/c/2", "Two")]))
        .await
        .unwrap();
    // `CanCreateTask` (baseunits/WebsiteModules.pas:414-420) keeps it waiting.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let listed = manager.list().await.unwrap();
    let status = |id| listed.iter().find(|t| t.task.id == id).unwrap().task.status;
    assert_eq!(status(first), TaskStatus::Downloading);
    assert_eq!(status(second), TaskStatus::Waiting);
    assert!(!f.transport.requests().iter().any(|r| r.contains("/c/2")));

    // Stopping the first frees the module's slot (`TTaskThread.Destroy` runs
    // `CheckAndActiveTask`, baseunits/uDownloadsManager.pas:509-517, :712-715).
    manager.stop(first).await.unwrap();
    let mut order = Vec::new();
    let wait = async {
        loop {
            if let EngineEvent::Status { task, status, .. } = events.recv().await.unwrap() {
                order.push((task, status));
                if task == second && status == TaskStatus::Finished {
                    return;
                }
            }
        }
    };
    tokio::time::timeout(PATIENCE, wait).await.unwrap();
    let stopped = order
        .iter()
        .position(|e| *e == (first, TaskStatus::Stopped))
        .unwrap();
    let started = order
        .iter()
        .position(|e| *e == (second, TaskStatus::Preparing))
        .unwrap();
    assert!(stopped < started, "{order:?}");
}

/// How soon a stopped task must be Stopped.
const STOP_BOUND: Duration = Duration::from_secs(2);

/// Stops task `id` and returns the statuses it goes through until Stopped, failing when that
/// takes longer than [`STOP_BOUND`].
async fn stop_promptly(
    manager: &DownloadManager,
    events: &mut broadcast::Receiver<EngineEvent>,
    id: TaskId,
) -> Vec<TaskStatus> {
    let started = std::time::Instant::now();
    manager.stop(id).await.unwrap();
    let seen = statuses(events, id, &[TaskStatus::Stopped]).await;
    assert!(
        started.elapsed() < STOP_BOUND,
        "took {:?}",
        started.elapsed()
    );
    seen
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_task_mid_download_cuts_its_requests_short() {
    let f = Fixture::new(&[("T.lua", T)], json!({}));
    f.transport.hold("/img/");
    let manager = f.manager().await;
    let mut events = manager.subscribe();
    let id = manager
        .add_task(f.download("t", &[("/c/3", "One")]))
        .await
        .unwrap();
    eventually("a page request", || {
        f.transport.count("https://t/img/c/3/1") == 1
    })
    .await;

    let seen = stop_promptly(&manager, &mut events, id).await;
    assert_eq!(seen.last(), Some(&TaskStatus::Stopped));
    let task = f.db.tasks().get(id).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Stopped);
}

/// A module that waits a minute in `OnGetPageNumber`.
const SLOW: &str = r#"
function Init() local m = NewWebsiteModule(); m.ID='s'; m.Name='S'; m.RootURL='https://s'; m.OnGetPageNumber='GPN' end
function GPN() sleep(60000); TASK.PageLinks.Add('https://s/img/1'); return true end
"#;

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_task_cuts_a_lua_wait_short() {
    let f = Fixture::new(&[("S.lua", SLOW)], json!({}));
    let manager = f.manager().await;
    let mut events = manager.subscribe();
    let id = manager
        .add_task(f.download("s", &[("/c", "One")]))
        .await
        .unwrap();
    statuses(&mut events, id, &[TaskStatus::Preparing]).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let seen = stop_promptly(&manager, &mut events, id).await;
    assert_eq!(seen, [TaskStatus::Stopped]);
    // Stopped mid-way, no pages were kept (baseunits/uDownloadsManager.pas:1197-1202).
    assert!(f.db.tasks().pages(id, 0).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chapter_already_packed_is_not_downloaded_again() {
    let f = Fixture::new(&[("T.lua", T)], json!({"output": {"format": "cbz"}}));
    let archive = f.out().join("Manga/One.cbz");
    fs::create_dir_all(archive.parent().unwrap()).unwrap();
    fs::write(&archive, "packed before").unwrap();
    let manager = f.manager().await;
    let mut events = manager.subscribe();

    let id = manager
        .add_task(f.download("t", &[("/c/2", "One")]))
        .await
        .unwrap();

    // Every page counts as downloaded when the chapter's archive exists
    // (`CheckForExists`, baseunits/uDownloadsManager.pas:1027-1041), and packing nothing
    // keeps the archive (baseunits/uPacker.pas:294-302).
    use TaskStatus::*;
    assert_eq!(
        statuses(&mut events, id, &[Finished, Failed]).await,
        [
            Waiting,
            Preparing,
            Downloading,
            Converting,
            Compressing,
            Finished
        ]
    );
    assert!(!f.transport.requests().iter().any(|r| r.contains("/img/")));
    assert_eq!(fs::read_to_string(&archive).unwrap(), "packed before");
    assert!(!f.out().join("Manga/One").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn progress_is_reported_per_chapter_with_pages_and_bytes() {
    let f = Fixture::new(&[("T.lua", T)], json!({}));
    let manager = f.manager().await;
    let mut events = manager.subscribe();
    let id = manager
        .add_task(f.download("t", &[("/c/2", "One"), ("/c/3", "Two")]))
        .await
        .unwrap();

    let mut last = HashMap::new();
    let wait = async {
        loop {
            match events.recv().await.unwrap() {
                EngineEvent::Progress(p) if p.task == id => {
                    last.insert(p.chapter, p);
                }
                EngineEvent::Status { task, status, .. }
                    if task == id && status == TaskStatus::Finished =>
                {
                    return;
                }
                _ => {}
            }
        }
    };
    tokio::time::timeout(PATIENCE, wait).await.unwrap();
    // `DownCounter`/`PageNumber` (baseunits/uDownloadsManager.pas:449-458).
    let done = |chapter: u32| (last[&chapter].pages_done, last[&chapter].pages_total);
    assert_eq!(done(0), (2, 2));
    assert_eq!(done(1), (3, 3));
    assert_eq!(last[&1].bytes, 5 * PNG.len() as u64);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_chapter_folders_only_the_chapters_pages_are_packed() {
    let settings = json!({
        "output": {"format": "cbz"},
        "saveto": {"generate_chapter_folder": false},
    });
    let f = Fixture::new(&[("T.lua", T)], settings);
    let manga = f.out().join("Manga");
    fs::create_dir_all(&manga).unwrap();
    fs::write(manga.join("cover.png"), PNG).unwrap();
    let manager = f.manager().await;
    let mut events = manager.subscribe();

    let id = manager
        .add_task(f.download("t", &[("/c/2", "One")]))
        .await
        .unwrap();

    let seen = statuses(&mut events, id, &[TaskStatus::Finished, TaskStatus::Failed]).await;
    assert_eq!(seen.last(), Some(&TaskStatus::Finished));
    // `Compress` packs the files of the chapter's pages only
    // (baseunits/uDownloadsManager.pas:579-590).
    assert_eq!(entries(&manga.join("One.cbz")), ["001.png", "002.png"]);
    assert!(manga.join("cover.png").exists());
}
