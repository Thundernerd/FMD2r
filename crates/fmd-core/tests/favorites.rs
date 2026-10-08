//! The favorites checker through the public `FavoritesChecker` API, with a real `WorkerPool`
//! running a fixture module whose stub site lists chapters, and a temp `app.db`
//! (docs/tickets/T25-library-favorites.md, "Seams under test").
//!
//! Expected behaviour comes from FMD2's `TFavoriteThread.DoCheck`/`DoCheckMissing`
//! (baseunits/uFavoritesManager.pas:329-531) and `TFavoriteManager.ShowResult` (:954-1178).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};

use fmd_core::download::{DownloadManager, EngineConfig};
use fmd_core::favorites::{
    CheckError, CheckMode, CheckScope, CheckerConfig, CheckerEvent, FavoritesChecker,
    FavoritesEventKind, TaskQueue,
};
use fmd_core::jobs::{Job, JobPhase, JobRegistry};
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_store::{AppDb, EventQuery, FavoriteId, NewFavorite};
use serde_json::{Value, json};

/// Answers each URL from a table (404 with an empty body for anything else) and counts requests.
#[derive(Default)]
struct Site {
    pages: Mutex<HashMap<String, String>>,
    requests: Mutex<Vec<String>>,
    /// While set, requests wait for it to be released.
    gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
}

impl Site {
    /// Holds every request until [`Site::open`].
    fn close(&self) {
        *self.gate.lock().unwrap() = Some(Arc::new(tokio::sync::Semaphore::new(0)));
    }

    fn open(&self) {
        if let Some(gate) = self.gate.lock().unwrap().take() {
            gate.add_permits(1000);
        }
    }
    fn set(&self, path: &str, body: &str) {
        let url = format!("https://site.test{path}");
        self.pages.lock().unwrap().insert(url, body.into());
    }

    fn requested(&self, path: &str) -> usize {
        let url = format!("https://site.test{path}");
        let requests = self.requests.lock().unwrap();
        requests.iter().filter(|u| **u == url).count()
    }
}

impl Transport for Site {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.requests.lock().unwrap().push(request.url.clone());
        let body = self.pages.lock().unwrap().get(&request.url).cloned();
        let response = WireResponse {
            status: if body.is_some() { 200 } else { 404 },
            reason: String::new(),
            headers: Vec::new(),
            body: body.unwrap_or_default().into_bytes(),
        };
        let gate = self.gate.lock().unwrap().clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _ = gate.acquire().await;
            }
            Ok(response)
        })
    }
}

/// A module whose series page holds a status line, then one `link name` chapter per line.
const SITE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'site'; m.Name = 'Site'; m.RootURL = 'https://site.test'
  m.OnGetInfo = 'GetInfo'
end

function GetInfo()
  if not HTTP.GET(MANGAINFO.URL) then return net_problem end
  local first = true
  for line in HTTP.Document.ToString():gmatch('[^\n]+') do
    if first then
      MANGAINFO.Title = 'Manga'
      MANGAINFO.Status = line
      first = false
    else
      local link, name = line:match('^(%S+) (.+)$')
      MANGAINFO.ChapterLinks.Add(link)
      MANGAINFO.ChapterNames.Add(name)
    end
  end
  return no_error
end
"#;

struct Fixture {
    dir: tempfile::TempDir,
    db: AppDb,
    site: Arc<Site>,
    registry: Arc<ModuleRegistry>,
    pool: Arc<WorkerPool>,
    http: HttpClient,
    settings: Arc<SettingsService>,
}

impl Fixture {
    /// `settings` is a merge patch over the default settings.
    fn new(settings: Value) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let lua = dir.path().join("lua");
        fs::create_dir_all(lua.join("modules")).unwrap();
        fs::write(lua.join("modules/Site.lua"), SITE).unwrap();
        let report = ModuleRegistry::load_dir(&lua);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let site = Arc::new(Site::default());
        let http = HttpClient::with_transport(site.clone()).unwrap();
        let mut config = PoolConfig::new(http.clone());
        config.threads = 4;
        config.lua_dir = lua;
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let service = SettingsService::load(db.clone()).unwrap();
        service.update(settings).unwrap();
        Fixture {
            db,
            site,
            registry: Arc::new(report.registry),
            pool: Arc::new(WorkerPool::new(config).unwrap()),
            http,
            settings: Arc::new(service),
            dir,
        }
    }

    /// A favorite of `/manga`, whose page lists chapters `/c1` to `/c3`, with the first two
    /// downloaded.
    fn favorite(&self) -> FavoriteId {
        self.site
            .set("/manga", "1\n/c1 Chapter 1\n/c2 Chapter 2\n/c3 Chapter 3");
        self.db
            .downloaded_chapters()
            .mark("site", "/manga", &["/c1", "/c2"])
            .unwrap();
        let favorite = self
            .db
            .favorites()
            .create(&NewFavorite {
                module_id: "site".into(),
                link: "/manga".into(),
                title: "Manga".into(),
                save_to: self.dir.path().join("out").to_string_lossy().into_owned(),
                cover_url: None,
            })
            .unwrap();
        favorite.id
    }

    async fn manager(&self) -> Arc<DownloadManager> {
        let config = EngineConfig::new(
            self.db.clone(),
            self.pool.clone(),
            self.registry.clone(),
            self.settings.clone(),
            self.http.clone(),
        );
        Arc::new(DownloadManager::open(config).await.unwrap())
    }

    fn checker(&self) -> FavoritesChecker {
        self.checker_with(None)
    }

    fn checker_with(&self, queue: Option<Arc<dyn TaskQueue>>) -> FavoritesChecker {
        self.checker_events(queue, |_| {})
    }

    fn checker_events(
        &self,
        queue: Option<Arc<dyn TaskQueue>>,
        on_event: impl Fn(CheckerEvent) + Send + Sync + 'static,
    ) -> FavoritesChecker {
        let registry = self.registry.clone();
        FavoritesChecker::new(
            CheckerConfig {
                db: self.db.clone(),
                pool: self.pool.clone(),
                modules: Arc::new(move |id| registry.get(id).cloned()),
                settings: self.settings.clone(),
                queue,
                jobs: JobRegistry::new(),
            },
            on_event,
        )
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chapter_on_the_site_but_not_downloaded_is_new() {
    let fx = Fixture::new(json!({}));
    let id = fx.favorite();

    let report = fx
        .checker()
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    assert_eq!(report.checked, 1);
    assert_eq!(report.found.len(), 1);
    assert_eq!(report.found[0].favorite, id);
    let links: Vec<&str> = report.found[0]
        .chapters
        .iter()
        .map(|c| c.link.as_str())
        .collect();
    assert_eq!(links, ["/c3"]);
    assert_eq!(fx.site.requested("/manga"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_check_stores_the_status_chapter_count_and_dates() {
    let fx = Fixture::new(json!({}));
    let id = fx.favorite();
    let before = fx.db.favorites().get(id).unwrap().unwrap();
    assert_eq!(before.date_last_checked, None);

    fx.checker()
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    let after = fx.db.favorites().get(id).unwrap().unwrap();
    // `CurrentChapter := ChapterLinks.Count`, `Status := NewMangaInfo.Status`
    // (baseunits/uFavoritesManager.pas:352-353).
    assert_eq!(after.current_chapter, 3);
    assert_eq!(after.status, "1");
    assert!(after.date_last_checked.is_some());
    // New chapters were found, so it counts as updated (:376-377).
    assert!(after.date_last_updated.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_favorite_without_new_chapters_is_checked_but_not_updated() {
    let fx = Fixture::new(json!({}));
    let id = fx.favorite();
    fx.db
        .downloaded_chapters()
        .mark("site", "/manga", &["/C3"])
        .unwrap();

    let report = fx
        .checker()
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    assert!(report.found.is_empty(), "{:?}", report.found);
    let after = fx.db.favorites().get(id).unwrap().unwrap();
    assert!(after.date_last_checked.is_some());
    assert_eq!(after.date_last_updated, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_auto_download_new_chapters_become_one_inbox_item() {
    let fx = Fixture::new(json!({ "favorites": { "auto_download": false } }));
    fx.favorite();

    fx.checker()
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    let events = fx.db.events().list(&EventQuery::default()).unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].kind, "new_chapters");
    // `RS_DlgNewChapterCaption`, `RS_LblNewChapterFound` and `RS_FavoriteHasNewChapter`
    // (baseunits/uFavoritesManager.pas:176-178, :986-990, :1063-1066).
    assert_eq!(events[0].title, "Found new chapter(s)");
    assert_eq!(
        events[0].body,
        json!("Found 1 new chapter from 1 manga(s):\n- Manga <Site> has 1 new chapter(s).")
    );
    assert!(fx.db.tasks().list().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_new_posts_nothing() {
    let fx = Fixture::new(json!({}));
    fx.favorite();
    fx.db
        .downloaded_chapters()
        .mark("site", "/manga", &["/c3"])
        .unwrap();

    fx.checker()
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    assert!(
        fx.db
            .events()
            .list(&EventQuery::default())
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_auto_download_new_chapters_are_queued_and_marked_downloaded() {
    let fx = Fixture::new(json!({
        "favorites": { "auto_download": true },
        "general": { "add_as_stopped": true },
    }));
    let id = fx.favorite();
    let manager = fx.manager().await;
    let checker = fx.checker_with(Some(manager.clone()));

    let report = checker
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    let tasks = fx.db.tasks().list().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(report.queued, [tasks[0].id]);
    // `DLManager.AddTask` with the favorite's link and title
    // (baseunits/uFavoritesManager.pas:1092-1099).
    assert_eq!(tasks[0].link, "/manga");
    assert_eq!(tasks[0].title, "Manga");
    let chapters = fx.db.tasks().chapters(tasks[0].id).unwrap();
    let links: Vec<&str> = chapters.iter().map(|c| c.link.as_str()).collect();
    assert_eq!(links, ["/c3"]);
    assert!(
        fx.db
            .events()
            .list(&EventQuery::default())
            .unwrap()
            .is_empty()
    );
    // Queued chapters join the downloaded list at once (:1122-1125), so the next check finds
    // nothing new.
    assert!(
        fx.db
            .downloaded_chapters()
            .contains("site", "/manga", "/c3")
            .unwrap()
    );
    let again = checker
        .check(CheckScope::Only(vec![id]), CheckMode::New)
        .await
        .unwrap();
    assert!(again.found.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_check_looks_for_chapter_files_not_the_downloaded_list() {
    let fx = Fixture::new(json!({ "saveto": { "chapter_rename": "%NUMBERING%" } }));
    fx.favorite();
    fx.db
        .downloaded_chapters()
        .mark("site", "/manga", &["/c3"])
        .unwrap();
    let out = fx.dir.path().join("out");
    fs::create_dir_all(out.join("0002")).unwrap();
    fs::write(out.join("0001.cbz"), b"").unwrap();
    fs::write(out.join("0002.cbz"), b"").unwrap();

    let report = fx
        .checker()
        .check(CheckScope::All, CheckMode::Missing)
        .await
        .unwrap();

    // CBZ files outnumber folders, so chapters are looked for as `<name>.cbz`; `0002` also has
    // a folder (packing was interrupted), so it counts as missing
    // (baseunits/uFavoritesManager.pas:425-511).
    let numbers: Vec<u32> = report.found[0].chapters.iter().map(|c| c.number).collect();
    assert_eq!(numbers, [2, 3]);
    let events = fx.db.events().list(&EventQuery::default()).unwrap();
    assert_eq!(events[0].kind, "missing_chapters");
    assert_eq!(
        events[0].body,
        json!(
            "Found 2 missing chapter(s) from 1 manga(s):\n- Manga <Site> has 2 missing chapter(s)."
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_commonest_chapter_file_type_decides_what_a_missing_check_looks_for() {
    let fx = Fixture::new(json!({
        "saveto": { "chapter_rename": "%NUMBERING%" },
        "output": { "format": "zip" },
    }));
    fx.favorite();
    let out = fx.dir.path().join("out");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("0001.zip"), b"").unwrap();
    fs::write(out.join("notes.txt"), b"").unwrap();

    let report = fx
        .checker()
        .check(CheckScope::All, CheckMode::Missing)
        .await
        .unwrap();

    // One ZIP and no other chapter file: ZIP wins; with none at all the output format would
    // (:442-472).
    let numbers: Vec<u32> = report.found[0].chapters.iter().map(|c| c.number).collect();
    assert_eq!(numbers, [2, 3]);
}

/// Turns the fixture's series into a completed one with nothing new.
fn complete(fx: &Fixture) {
    fx.site
        .set("/manga", "0\n/c1 Chapter 1\n/c2 Chapter 2\n/c3 Chapter 3");
    fx.db
        .downloaded_chapters()
        .mark("site", "/manga", &["/c3"])
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn completed_series_are_removed_when_the_setting_says_so() {
    let fx = Fixture::new(json!({ "favorites": { "remove_completed": true } }));
    let id = fx.favorite();
    complete(&fx);

    let report = fx
        .checker()
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    // `OptionAutoCheckFavRemoveCompletedManga`: a completed series with nothing new is removed
    // (baseunits/uFavoritesManager.pas:997-1043).
    assert_eq!(report.completed, [id]);
    assert_eq!(fx.db.favorites().get(id).unwrap(), None);
    let events = fx.db.events().list(&EventQuery::default()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "completed");
    assert_eq!(events[0].title, "Found completed manga(s)");
    assert_eq!(
        events[0].body,
        json!("1 completed manga(s) removed:\n- Manga <Site>")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn completed_series_stay_by_default() {
    let fx = Fixture::new(json!({}));
    let id = fx.favorite();
    complete(&fx);

    let report = fx
        .checker()
        .check(CheckScope::All, CheckMode::New)
        .await
        .unwrap();

    assert_eq!(report.completed, [id]);
    assert_eq!(fx.db.favorites().get(id).unwrap().unwrap().status, "0");
    assert!(
        fx.db
            .events()
            .list(&EventQuery::default())
            .unwrap()
            .is_empty()
    );
}

/// Waits for the event matching `wanted` on `events`.
async fn until(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<CheckerEvent>,
    wanted: impl Fn(&CheckerEvent) -> bool,
) -> CheckerEvent {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let event = events.recv().await.unwrap();
            if wanted(&event) {
                return event;
            }
        }
    })
    .await
    .unwrap()
}

fn job_kind(event: &CheckerEvent) -> Option<FavoritesEventKind> {
    match event {
        CheckerEvent::Job(e) => Some(e.kind),
        CheckerEvent::Inbox(_) => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_started_check_runs_in_the_background_and_never_overlaps() {
    let fx = Fixture::new(json!({}));
    fx.favorite();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let checker = fx.checker_events(None, move |e| {
        let _ = tx.send(e);
    });
    fx.site.close();

    checker.start(CheckScope::All, CheckMode::New).unwrap();
    until(&mut events, |e| {
        job_kind(e) == Some(FavoritesEventKind::Started)
    })
    .await;

    // `CheckForNewChapter` while running only says a check is running
    // (baseunits/uFavoritesManager.pas:860-866).
    assert!(matches!(
        checker.start(CheckScope::All, CheckMode::New),
        Err(CheckError::AlreadyRunning)
    ));
    assert!(matches!(
        Job::run(&checker),
        Err(fmd_core::jobs::JobError::AlreadyRunning)
    ));
    let status = checker.status();
    assert_eq!(status.phase, JobPhase::Running);
    assert_eq!((status.done, status.total), (0, 1));

    fx.site.open();
    let finished = until(&mut events, |e| {
        job_kind(e) == Some(FavoritesEventKind::Finished)
    })
    .await;
    let CheckerEvent::Job(finished) = finished else {
        unreachable!()
    };
    assert_eq!((finished.done, finished.total), (1, 1));
    assert_eq!(finished.new_chapters, Some(1));
    let status = checker.status();
    assert_eq!(status.phase, JobPhase::Done);
    assert_eq!((status.done, status.total), (1, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_check_reports_nothing() {
    let fx = Fixture::new(json!({}));
    fx.favorite();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let checker = fx.checker_events(None, move |e| {
        let _ = tx.send(e);
    });
    fx.site.close();
    checker.start(CheckScope::All, CheckMode::New).unwrap();
    until(&mut events, |e| {
        job_kind(e) == Some(FavoritesEventKind::Started)
    })
    .await;

    Job::cancel(&checker).unwrap();
    fx.site.open();

    until(&mut events, |e| {
        job_kind(e) == Some(FavoritesEventKind::Cancelled)
    })
    .await;
    // A terminated task skips `ShowResult` (baseunits/uFavoritesManager.pas:636-656).
    assert!(
        fx.db
            .events()
            .list(&EventQuery::default())
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn favorites_are_checked_on_at_most_the_configured_threads() {
    let fx = Fixture::new(json!({ "connections": { "max_favorite_threads": 2 } }));
    for link in ["/a", "/b", "/c"] {
        fx.site.set(link, "1\n/c1 Chapter 1");
        fx.db
            .favorites()
            .create(&NewFavorite {
                module_id: "site".into(),
                link: link.into(),
                title: link.into(),
                save_to: String::new(),
                cover_url: None,
            })
            .unwrap();
    }
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let checker = fx.checker_events(None, move |e| {
        let _ = tx.send(e);
    });
    fx.site.close();

    checker.start(CheckScope::All, CheckMode::New).unwrap();
    // `OptionMaxFavoriteThreads` (baseunits/uFavoritesManager.pas:717, :737).
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while fx.site.requests.lock().unwrap().len() < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(fx.site.requests.lock().unwrap().len(), 2);

    fx.site.open();
    let finished = until(&mut events, |e| {
        job_kind(e) == Some(FavoritesEventKind::Finished)
    })
    .await;
    let CheckerEvent::Job(finished) = finished else {
        unreachable!()
    };
    assert_eq!((finished.done, finished.total), (3, 3));
    assert_eq!(fx.site.requests.lock().unwrap().len(), 3);
}

/// Waits for the next finished run on `events`, without a timer of its own so the paused
/// clock jumps straight to the scheduler's.
async fn next_finished(events: &mut tokio::sync::mpsc::UnboundedReceiver<CheckerEvent>) {
    loop {
        if job_kind(&events.recv().await.unwrap()) == Some(FavoritesEventKind::Finished) {
            return;
        }
    }
}

#[tokio::test(start_paused = true)]
async fn the_schedule_checks_at_startup_then_every_interval() {
    let fx = Fixture::new(json!({ "favorites": {
        "check_at_startup": true,
        "check_on_interval": true,
        "check_interval_minutes": 1,
    } }));
    fx.favorite();
    fx.db
        .downloaded_chapters()
        .mark("site", "/manga", &["/c3"])
        .unwrap();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let checker = fx.checker_events(None, move |e| {
        let _ = tx.send(e);
    });
    let start = tokio::time::Instant::now();

    tokio::spawn(checker.clone().schedule());

    // `tmStartupTimer` checks at once (mangadownloader/forms/frmMain.pas:2078-2082) ...
    next_finished(&mut events).await;
    assert!(start.elapsed() < std::time::Duration::from_secs(60));
    assert!(checker.status().next_run.is_some());
    // ... and `tmCheckFavorites` an interval after the last check ended
    // (mangadownloader/forms/frmMain.pas:1871-1881, :6335; baseunits/uFavoritesManager.pas:597).
    next_finished(&mut events).await;
    assert!(start.elapsed() >= std::time::Duration::from_secs(60));
    assert_eq!(fx.site.requested("/manga"), 2);
}

#[tokio::test(start_paused = true)]
async fn without_the_startup_check_the_first_check_waits_an_interval() {
    let fx = Fixture::new(json!({ "favorites": {
        "check_at_startup": false,
        "check_interval_minutes": 5,
    } }));
    fx.favorite();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let checker = fx.checker_events(None, move |e| {
        let _ = tx.send(e);
    });
    let start = tokio::time::Instant::now();

    tokio::spawn(checker.schedule());

    next_finished(&mut events).await;
    assert!(start.elapsed() >= std::time::Duration::from_secs(300));
}
