//! The library's new-chapter check, mirroring FMD2's `TFavoriteManager`, `TFavoriteTask` and
//! `TFavoriteThread` (baseunits/uFavoritesManager.pas:302-1178).
//!
//! A run asks each enabled favorite's module for the series' info (`OnGetInfo`), diffs the
//! chapters against what was downloaded, stores the favorite's new status and chapter count, and
//! then queues what it found or reports it in the inbox.

use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fmd_lua::WorkerPool;
use fmd_store::{AppDb, Event, EventSeverity, Favorite, FavoriteId, NewEvent, StoreError};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;
use tokio::time::Instant;
use utoipa::ToSchema;

use fmd_pack::{RenameContext, custom_rename};

use crate::download::{
    ChapterSpec, DownloadManager, EngineError, ModuleLookup, NewDownload, TaskId, rename_options,
    save_to,
};
use crate::info::{InfoError, InfoOptions, MangaInfo, get_info};
use crate::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use crate::settings::{OutputFormat, SaveToSettings, SettingsService};

/// `MangaInfo_StatusCompleted` (baseunits/uBaseUnit.pas:230).
const STATUS_COMPLETED: &str = "0";

/// What the checker runs on.
#[derive(Clone)]
pub struct CheckerConfig {
    pub db: AppDb,
    /// Runs the modules' `OnGetInfo`.
    pub pool: Arc<WorkerPool>,
    pub modules: Arc<ModuleLookup>,
    pub settings: Arc<SettingsService>,
    /// Where found chapters are queued when `favorites.auto_download` is on; without one they
    /// are reported in the inbox.
    pub queue: Option<Arc<dyn TaskQueue>>,
    /// Where the `favorites` job announces its changes.
    pub jobs: JobRegistry,
}

/// What [`TaskQueue::add_task`] returns.
pub type QueueFuture<'a> = Pin<Box<dyn Future<Output = Result<TaskId, EngineError>> + Send + 'a>>;

/// Where found chapters are queued: the [`DownloadManager`].
pub trait TaskQueue: Send + Sync + 'static {
    /// Queues `download` as a new task.
    fn add_task(&self, download: NewDownload) -> QueueFuture<'_>;
}

impl TaskQueue for DownloadManager {
    fn add_task(&self, download: NewDownload) -> QueueFuture<'_> {
        Box::pin(DownloadManager::add_task(self, download))
    }
}

/// The directory a series added to the library from `info` downloads to: `dir`, or the default
/// download directory when empty, plus the manga folder when one is generated and not already
/// part of it (`btAddToFavoritesClick`, mangadownloader/forms/frmMain.pas:2804-2827).
pub fn favorite_save_to(
    saveto: &SaveToSettings,
    website: &str,
    info: &MangaInfo,
    dir: &str,
) -> String {
    let download = NewDownload {
        title: info.title.clone(),
        authors: info.authors.clone(),
        artists: info.artists.clone(),
        save_to: dir.to_owned(),
        ..NewDownload::default()
    };
    save_to(saveto, website, &download)
}

/// Which favorites a run checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckScope {
    /// Every enabled favorite.
    All,
    /// These favorites, when enabled.
    Only(Vec<FavoriteId>),
}

/// What a run looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckMode {
    /// Chapters on the site that are not downloaded (`DoCheck`,
    /// baseunits/uFavoritesManager.pas:329-395).
    New,
    /// Chapters on the site whose file or folder is missing from the favorite's directory
    /// (`DoCheckMissing`, baseunits/uFavoritesManager.pas:397-531).
    Missing,
}

/// A chapter a run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundChapter {
    pub name: String,
    /// Relative to the module's `RootURL`.
    pub link: String,
    /// 1-based position in the series' chapter list, FMD2's `%NUMBERING%`.
    pub number: u32,
}

/// The chapters a run found for one favorite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteFound {
    pub favorite: FavoriteId,
    pub title: String,
    pub module_id: String,
    /// The module's name, FMD2's `Website`.
    pub website: String,
    pub link: String,
    pub save_to: String,
    /// As the module reports them, for the chapter names.
    pub authors: String,
    pub artists: String,
    pub chapters: Vec<FoundChapter>,
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckReport {
    /// Favorites whose info was read.
    pub checked: u64,
    /// Favorites with new (or missing) chapters.
    pub found: Vec<FavoriteFound>,
    /// Completed series with nothing new (`MangaInfo_StatusCompleted`); removed when
    /// `favorites.remove_completed` is on.
    pub completed: Vec<FavoriteId>,
    /// The tasks the found chapters were queued in.
    pub queued: Vec<TaskId>,
    /// The inbox item reporting what was found, when it was not queued.
    pub inbox: Option<Event>,
    /// The run was cancelled before it reported anything.
    pub cancelled: bool,
}

/// What happened to a check run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FavoritesEventKind {
    Started,
    /// A favorite was checked.
    Progress,
    Finished,
    Cancelled,
    Failed,
}

/// One step of a check run, for the Library's progress (`job.favorites.<kind>` events).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct FavoritesEvent {
    pub kind: FavoritesEventKind,
    pub mode: CheckMode,
    /// Favorites checked so far.
    pub done: u64,
    /// Favorites to check.
    pub total: u64,
    /// The favorite just checked (`progress`).
    pub favorite_id: Option<i64>,
    /// New (or missing) chapters found, once finished.
    pub new_chapters: Option<u64>,
    /// Why it failed.
    pub error: Option<String>,
}

impl FavoritesEvent {
    fn new(kind: FavoritesEventKind, mode: CheckMode, done: u64, total: u64) -> Self {
        Self {
            kind,
            mode,
            done,
            total,
            favorite_id: None,
            new_chapters: None,
            error: None,
        }
    }
}

/// What the checker reports while it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckerEvent {
    Job(FavoritesEvent),
    /// A new inbox item was stored.
    Inbox(Event),
}

/// Why a run could not run.
#[derive(Debug, Error)]
pub enum CheckError {
    #[error("a favorites check is already running")]
    AlreadyRunning,
    #[error("no async runtime to run the check on")]
    NoRuntime,
    #[error("store: {0}")]
    Store(#[from] StoreError),
    /// Blocking work on the runtime's thread pool panicked or was cancelled.
    #[error("background work: {0}")]
    Join(String),
}

/// Checks favorites for new chapters (`TFavoriteManager`), one run at a time, as the
/// `favorites` background job. Cheap to clone; clones share the runs.
#[derive(Clone)]
pub struct FavoritesChecker {
    inner: Arc<Inner>,
}

struct Inner {
    config: CheckerConfig,
    on_event: Box<dyn Fn(CheckerEvent) + Send + Sync>,
    status: Mutex<JobStatus>,
    /// `Terminated`: set to stop the run going on.
    cancelled: AtomicBool,
    /// Counts the runs that ended, so a scheduled check can wait for a manual one.
    ended: watch::Sender<u64>,
}

/// A completed series with nothing new.
struct CompletedSeries {
    id: FavoriteId,
    title: String,
    website: String,
}

/// What checking one favorite found.
enum Checked {
    Found(FavoriteFound),
    Completed(CompletedSeries),
    Nothing,
}

impl FavoritesChecker {
    /// The job's id in `/api/jobs/{id}`.
    pub const ID: &str = "favorites";

    /// A checker passing every event to `on_event`.
    pub fn new(
        config: CheckerConfig,
        on_event: impl Fn(CheckerEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                on_event: Box::new(on_event),
                status: Mutex::new(JobStatus {
                    phase: JobPhase::Idle,
                    done: 0,
                    total: 0,
                    last_run: None,
                    next_run: None,
                    last_error: None,
                }),
                cancelled: AtomicBool::new(false),
                ended: watch::channel(0).0,
            }),
        }
    }

    /// Starts checking the favorites in `scope` in the background, unless a run is going
    /// (`CheckForNewChapter`/`CheckForMissingChapters`, baseunits/uFavoritesManager.pas:832-928).
    pub fn start(&self, scope: CheckScope, mode: CheckMode) -> Result<(), CheckError> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| CheckError::NoRuntime)?;
        self.begin()?;
        let this = self.clone();
        runtime.spawn(async move {
            let result = this.run_checks(scope, mode).await;
            this.end(mode, &result);
        });
        Ok(())
    }

    /// Checks the favorites in `scope` and waits for the run to end, unless a run is going.
    pub async fn check(
        &self,
        scope: CheckScope,
        mode: CheckMode,
    ) -> Result<CheckReport, CheckError> {
        self.begin()?;
        let result = self.run_checks(scope, mode).await;
        self.end(mode, &result);
        result
    }

    /// Runs the checks FMD2's timers run, following setting changes: one at startup when
    /// `favorites.check_at_startup` is on (`tmStartupTimer`,
    /// mangadownloader/forms/frmMain.pas:2078-2082), then one every
    /// `favorites.check_interval_minutes` after the last ended while
    /// `favorites.check_on_interval` is on (`tmCheckFavorites`, :1871-1881, :6335, re-armed when
    /// a check ends, baseunits/uFavoritesManager.pas:597). A check already going when one is due
    /// counts as that one, and the interval runs from its end, so runs never overlap. Never
    /// returns.
    pub async fn schedule(self) {
        let settings = self.inner.config.settings.clone();
        let mut changes = settings.subscribe();
        if settings.get().favorites.check_at_startup {
            self.scheduled_check().await;
        }
        let mut last = Instant::now();
        let mut watching = true;
        loop {
            let favorites = settings.get().favorites.clone();
            let interval =
                Duration::from_secs(u64::from(favorites.check_interval_minutes.max(1)) * 60);
            let next = favorites.check_on_interval.then(|| last + interval);
            self.set_next_run(next.map(unix_ms));
            let due = async {
                match next {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                () = due => {
                    self.scheduled_check().await;
                    last = Instant::now();
                }
                changed = changes.changed(), if watching => {
                    // The settings service outlives the schedule; without it, keep the schedule.
                    watching = changed.is_ok();
                }
            }
        }
    }

    /// A scheduled check of every favorite (`isAuto`), waiting for it to end. A check already
    /// going (started from the API) counts as this one: it waits for that to end instead.
    async fn scheduled_check(&self) {
        let mut ended = self.inner.ended.subscribe();
        ended.mark_unchanged();
        // A failed run has been logged.
        if let Err(CheckError::AlreadyRunning) = self.check(CheckScope::All, CheckMode::New).await {
            // The sender lives in `self`, so this only returns once a run ended.
            let _ = ended.changed().await;
        }
    }

    /// Records when the scheduler runs the job next, in Unix milliseconds.
    fn set_next_run(&self, at: Option<i64>) {
        self.update(|status| status.next_run = at);
    }

    /// Marks a run as going, unless one already is (`isRunning`).
    fn begin(&self) -> Result<(), CheckError> {
        {
            let mut status = lock(&self.inner.status);
            if status.phase == JobPhase::Running {
                return Err(CheckError::AlreadyRunning);
            }
            status.phase = JobPhase::Running;
            status.done = 0;
            status.total = 0;
            status.last_run = Some(now_ms());
            status.last_error = None;
        }
        self.inner.cancelled.store(false, Ordering::SeqCst);
        self.inner.config.jobs.changed(Self::ID);
        Ok(())
    }

    /// Ends the run with `result`.
    fn end(&self, mode: CheckMode, result: &Result<CheckReport, CheckError>) {
        let (kind, new_chapters, error) = match result {
            Ok(report) if report.cancelled => (FavoritesEventKind::Cancelled, None, None),
            Ok(report) => {
                let chapters = report.found.iter().map(|f| f.chapters.len() as u64).sum();
                (FavoritesEventKind::Finished, Some(chapters), None)
            }
            Err(e) => {
                tracing::warn!(target: "fmd_core", "favorites check: {e}");
                (FavoritesEventKind::Failed, None, Some(e.to_string()))
            }
        };
        let (done, total) = {
            let mut status = lock(&self.inner.status);
            status.phase = if error.is_some() {
                JobPhase::Failed
            } else {
                JobPhase::Done
            };
            status.last_error = error.clone();
            (status.done, status.total)
        };
        self.inner.config.jobs.changed(Self::ID);
        self.inner.ended.send_modify(|n| *n += 1);
        self.emit(FavoritesEvent {
            new_chapters,
            error,
            ..FavoritesEvent::new(kind, mode, done, total)
        });
    }

    fn emit(&self, event: FavoritesEvent) {
        (self.inner.on_event)(CheckerEvent::Job(event));
    }

    fn update(&self, change: impl FnOnce(&mut JobStatus)) {
        change(&mut lock(&self.inner.status));
        self.inner.config.jobs.changed(Self::ID);
    }

    /// `TFavoriteTask.Execute` (baseunits/uFavoritesManager.pas:607-667): checks the favorites
    /// on at most `connections.max_favorite_threads` at once (`GetNext`, :711-742), then shows
    /// the result unless it was terminated.
    async fn run_checks(
        &self,
        scope: CheckScope,
        mode: CheckMode,
    ) -> Result<CheckReport, CheckError> {
        let favorites = self.favorites(scope).await?;
        let total = favorites.len() as u64;
        self.update(|status| status.total = total);
        self.emit(FavoritesEvent::new(
            FavoritesEventKind::Started,
            mode,
            0,
            total,
        ));
        let threads = self
            .inner
            .config
            .settings
            .get()
            .connections
            .max_favorite_threads
            .max(1);
        let permits = Arc::new(Semaphore::new(threads as usize));
        let mut running = JoinSet::new();
        for (index, favorite) in favorites.into_iter().enumerate() {
            let permit = permits.clone().acquire_owned().await;
            let this = self.clone();
            running.spawn(async move {
                let _permit = permit;
                if this.inner.cancelled.load(Ordering::SeqCst) {
                    return (index, Ok(None));
                }
                let checked = this.check_one(&favorite, mode).await;
                let done = {
                    let mut status = lock(&this.inner.status);
                    status.done += 1;
                    status.done
                };
                this.inner.config.jobs.changed(Self::ID);
                this.emit(FavoritesEvent {
                    favorite_id: Some(favorite.id.0),
                    ..FavoritesEvent::new(FavoritesEventKind::Progress, mode, done, total)
                });
                (index, checked)
            });
        }
        let mut results = Vec::new();
        while let Some(joined) = running.join_next().await {
            match joined {
                Ok(result) => results.push(result),
                Err(e) => tracing::warn!(target: "fmd_core", "favorites check: {e}"),
            }
        }
        // In library order, as FMD2 lists them.
        results.sort_by_key(|(index, _)| *index);
        let mut report = CheckReport::default();
        let mut completed = Vec::new();
        for (_, result) in results {
            let checked = match result {
                Ok(Some(checked)) => checked,
                Ok(None) => continue,
                // One favorite's failure leaves the others' results standing, as FMD2's
                // per-favorite `ExceptionHandle` (baseunits/uFavoritesManager.pas:391-394).
                Err(e) => {
                    tracing::warn!(target: "fmd_core", "favorites check: {e}");
                    continue;
                }
            };
            report.checked += 1;
            match checked {
                Checked::Found(found) => report.found.push(found),
                Checked::Completed(series) => {
                    report.completed.push(series.id);
                    completed.push(series);
                }
                Checked::Nothing => {}
            }
        }
        if self.inner.cancelled.load(Ordering::SeqCst) {
            report.cancelled = true;
            return Ok(report);
        }
        self.remove_completed(&completed).await?;
        self.show_result(&mut report, mode).await?;
        Ok(report)
    }

    /// Checks one favorite (`DoCheck`/`DoCheckMissing`, baseunits/uFavoritesManager.pas:329-531).
    /// A favorite whose info cannot be read is logged and skipped (`None`), as FMD2's
    /// `ExceptionHandle`.
    async fn check_one(
        &self,
        favorite: &Favorite,
        mode: CheckMode,
    ) -> Result<Option<Checked>, CheckError> {
        let info = match self.info(favorite).await {
            Ok(info) => info,
            Err(e) => {
                tracing::warn!(target: "fmd_core", "checking favorite {}: {e}", favorite.title);
                return Ok(None);
            }
        };
        let chapters = match mode {
            CheckMode::New => self.new_chapters(favorite, &info).await?,
            CheckMode::Missing => self.missing_chapters(favorite, &info).await?,
        };
        if self.inner.cancelled.load(Ordering::SeqCst) {
            return Ok(None);
        }
        // The favorite as stored now: it may have been edited, or removed, during the check.
        let Some(favorite) = self
            .store_checked(favorite.id, &info, !chapters.is_empty())
            .await?
        else {
            return Ok(None);
        };
        let website = self.website(&favorite.module_id);
        if !chapters.is_empty() {
            return Ok(Some(Checked::Found(FavoriteFound {
                favorite: favorite.id,
                title: favorite.title,
                module_id: favorite.module_id,
                website,
                link: favorite.link,
                save_to: favorite.save_to,
                authors: info.authors,
                artists: info.artists,
                chapters,
            })));
        }
        // Only a new-chapter check keeps a completed series' info (:380-385, :513-517).
        if mode == CheckMode::New && info.status == STATUS_COMPLETED {
            return Ok(Some(Checked::Completed(CompletedSeries {
                id: favorite.id,
                title: favorite.title,
                website,
            })));
        }
        Ok(Some(Checked::Nothing))
    }

    /// The module's name, or its ID when it is gone.
    fn website(&self, module_id: &str) -> String {
        (self.inner.config.modules)(module_id)
            .map_or_else(|| module_id.to_owned(), |m| m.def().name)
    }

    /// Removes `completed` when `favorites.remove_completed` is on, and
    /// reports it in the inbox in place of FMD2's confirmation dialog (`ShowResult`,
    /// baseunits/uFavoritesManager.pas:997-1043).
    async fn remove_completed(&self, completed: &[CompletedSeries]) -> Result<(), CheckError> {
        if completed.is_empty() || !self.inner.config.settings.get().favorites.remove_completed {
            return Ok(());
        }
        let ids: Vec<FavoriteId> = completed.iter().map(|c| c.id).collect();
        let db = self.inner.config.db.clone();
        blocking(move || ids.iter().try_for_each(|id| db.favorites().delete(*id))).await?;
        let mut body = format!("{} completed manga(s) removed:", completed.len());
        for c in completed {
            body.push_str(&format!("\n- {} <{}>", c.title, c.website));
        }
        self.notify(NewEvent {
            kind: "completed".into(),
            severity: EventSeverity::Info,
            module_id: None,
            task_id: None,
            title: "Found completed manga(s)".into(),
            body: serde_json::Value::String(body),
        })
        .await?;
        Ok(())
    }

    /// Stores `event` in the inbox and announces it.
    async fn notify(&self, event: NewEvent) -> Result<Event, CheckError> {
        let db = self.inner.config.db.clone();
        let stored = blocking(move || db.events().push(&event)).await?;
        (self.inner.on_event)(CheckerEvent::Inbox(stored.clone()));
        Ok(stored)
    }

    /// Queues what the run found when `favorites.auto_download` is on, or else reports it in
    /// one inbox item in place of FMD2's new-chapter dialog (`ShowResult`,
    /// baseunits/uFavoritesManager.pas:1047-1073).
    async fn show_result(
        &self,
        report: &mut CheckReport,
        mode: CheckMode,
    ) -> Result<(), CheckError> {
        if report.found.is_empty() {
            return Ok(());
        }
        let auto_download = self.inner.config.settings.get().favorites.auto_download;
        match &self.inner.config.queue {
            Some(queue) if auto_download => {
                for found in &report.found {
                    if let Some(task) = self.queue(queue.as_ref(), found).await? {
                        report.queued.push(task);
                    }
                }
            }
            _ => {
                report.inbox = Some(self.notify(inbox_item(&report.found, mode)).await?);
            }
        }
        Ok(())
    }

    /// Queues `found`'s chapters as a new task and adds them to the downloaded list at once
    /// (`ShowResult`, baseunits/uFavoritesManager.pas:1087-1129). A task that cannot be queued
    /// is logged; its chapters stay new, so the next check finds them again.
    async fn queue(
        &self,
        queue: &dyn TaskQueue,
        found: &FavoriteFound,
    ) -> Result<Option<TaskId>, CheckError> {
        let download = NewDownload {
            module_id: found.module_id.clone(),
            manga_link: found.link.clone(),
            title: found.title.clone(),
            authors: found.authors.clone(),
            artists: found.artists.clone(),
            chapters: found
                .chapters
                .iter()
                .map(|c| ChapterSpec {
                    link: c.link.clone(),
                    title: c.name.clone(),
                    number: c.number,
                })
                .collect(),
            save_to: found.save_to.clone(),
        };
        let task = match queue.add_task(download).await {
            Ok(task) => task,
            Err(e) => {
                tracing::warn!(target: "fmd_core", "queueing new chapters of {}: {e}", found.title);
                return Ok(None);
            }
        };
        let db = self.inner.config.db.clone();
        let (module, link) = (found.module_id.clone(), found.link.clone());
        let chapters: Vec<String> = found.chapters.iter().map(|c| c.link.clone()).collect();
        blocking(move || {
            let links: Vec<&str> = chapters.iter().map(String::as_str).collect();
            db.downloaded_chapters().mark(&module, &link, &links)
        })
        .await?;
        Ok(Some(task))
    }

    /// Stores what a check learned: the chapter count and status at once (`DoCheck`,
    /// baseunits/uFavoritesManager.pas:351-353), the check time, and the update time when it
    /// found chapters (:373-378). The other fields are left as stored now, so edits made during
    /// the check stay. Returns the updated favorite, or `None` when it was removed meanwhile.
    async fn store_checked(
        &self,
        id: FavoriteId,
        info: &MangaInfo,
        found: bool,
    ) -> Result<Option<Favorite>, CheckError> {
        let now = now_ms();
        let chapters = u32::try_from(info.chapters.len()).unwrap_or(u32::MAX);
        let status = info.status.clone();
        let db = self.inner.config.db.clone();
        // The repositories lock the connection per statement; this read-modify-write races
        // only with another write to the same favorite in between, which a PATCH would redo.
        blocking(move || {
            let Some(mut favorite) = db.favorites().get(id)? else {
                return Ok(None);
            };
            favorite.current_chapter = chapters;
            favorite.status = status;
            favorite.date_last_checked = Some(now);
            if found {
                favorite.date_last_updated = Some(now);
            }
            db.favorites().update(&favorite)?;
            Ok(Some(favorite))
        })
        .await
    }

    async fn favorites(&self, scope: CheckScope) -> Result<Vec<Favorite>, CheckError> {
        let db = self.inner.config.db.clone();
        let all = blocking(move || db.favorites().list()).await?;
        Ok(all
            .into_iter()
            .filter(|f| f.enabled && !f.link.trim().is_empty())
            .filter(|f| match &scope {
                CheckScope::All => true,
                CheckScope::Only(ids) => ids.contains(&f.id),
            })
            .collect())
    }

    async fn info(&self, favorite: &Favorite) -> Result<MangaInfo, InfoError> {
        let config = &self.inner.config;
        let module = (config.modules)(&favorite.module_id).ok_or(InfoError::UnknownModule)?;
        let options = InfoOptions {
            remove_manga_name_from_chapter: config
                .settings
                .get()
                .saveto
                .remove_manga_name_from_chapter,
        };
        get_info(&config.pool, &module, &favorite.link, options).await
    }

    /// The chapters of `info` that are not in the downloaded list (`DoCheck`,
    /// baseunits/uFavoritesManager.pas:357-371): FMD2 looks each link up in a sorted,
    /// case-insensitive `TStringList`.
    async fn new_chapters(
        &self,
        favorite: &Favorite,
        info: &MangaInfo,
    ) -> Result<Vec<FoundChapter>, CheckError> {
        let db = self.inner.config.db.clone();
        let (module, link) = (favorite.module_id.clone(), favorite.link.clone());
        let downloaded =
            blocking(move || db.downloaded_chapters().list_for(&module, &link)).await?;
        let downloaded: HashSet<String> = downloaded.iter().map(|l| l.to_lowercase()).collect();
        Ok(info
            .chapters
            .iter()
            .enumerate()
            .filter(|(_, c)| !downloaded.contains(&c.link.to_lowercase()))
            .map(|(i, c)| found_chapter(i, c))
            .collect())
    }

    /// The chapters of `info` with no file or folder in the favorite's directory
    /// (`DoCheckMissing`, baseunits/uFavoritesManager.pas:425-511): each chapter's name is made
    /// as a download would name it, and looked for as the kind of chapter the directory holds
    /// most of, or else as the output format says.
    async fn missing_chapters(
        &self,
        favorite: &Favorite,
        info: &MangaInfo,
    ) -> Result<Vec<FoundChapter>, CheckError> {
        if info.chapters.is_empty() {
            return Ok(Vec::new());
        }
        let settings = self.inner.config.settings.get();
        let website = self.website(&favorite.module_id);
        let options = rename_options(&settings.saveto);
        let names: Vec<String> = info
            .chapters
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let numbering = format!("{:04}", i + 1);
                let ctx = RenameContext {
                    website: &website,
                    manga: &favorite.title,
                    author: &info.authors,
                    artist: &info.artists,
                    chapter: &c.name,
                    numbering: &numbering,
                    filename: "",
                };
                custom_rename(&settings.saveto.chapter_rename, &ctx, &options)
            })
            .collect();
        let dir = favorite.save_to.clone();
        let fallback = settings.output.format;
        let missing = match tokio::task::spawn_blocking(move || {
            let dir = Path::new(&dir);
            let format = detect_format(dir).unwrap_or(fallback);
            names
                .iter()
                .map(|name| !chapter_exists(&dir.join(name), format))
                .collect::<Vec<bool>>()
        })
        .await
        {
            Ok(missing) => missing,
            Err(e) => return Err(CheckError::Join(e.to_string())),
        };
        Ok(info
            .chapters
            .iter()
            .enumerate()
            .zip(missing)
            .filter(|(_, missing)| *missing)
            .map(|((i, c), _)| found_chapter(i, c))
            .collect())
    }
}

fn found_chapter(index: usize, chapter: &crate::info::Chapter) -> FoundChapter {
    FoundChapter {
        name: chapter.name.clone(),
        link: chapter.link.clone(),
        number: u32::try_from(index + 1).unwrap_or(u32::MAX),
    }
}

/// The kind of chapter `dir` holds most of: folders, unless more `.cbz`, `.zip`, `.pdf` or
/// `.epub` files (checked in that order, a later kind winning only with more); `None` when it
/// holds none (`DoCheckMissing`, baseunits/uFavoritesManager.pas:430-473).
fn detect_format(dir: &Path) -> Option<OutputFormat> {
    let (mut dirs, mut cbz, mut zip, mut pdf, mut epub) = (0, 0, 0, 0, 0);
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            dirs += 1;
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        match Path::new(&name).extension().and_then(|e| e.to_str()) {
            Some("cbz") => cbz += 1,
            Some("zip") => zip += 1,
            Some("pdf") => pdf += 1,
            Some("epub") => epub += 1,
            _ => {}
        }
    }
    if dirs + cbz + zip + pdf + epub == 0 {
        return None;
    }
    let mut format = OutputFormat::Folder;
    let mut max = dirs;
    for (count, kind) in [
        (cbz, OutputFormat::Cbz),
        (zip, OutputFormat::Zip),
        (pdf, OutputFormat::Pdf),
        (epub, OutputFormat::Epub),
    ] {
        if count > max {
            max = count;
            format = kind;
        }
    }
    Some(format)
}

/// Whether the chapter at `path` (without extension) was downloaded as `format`. A folder next
/// to an archive means packing was interrupted, so it counts as missing
/// (baseunits/uFavoritesManager.pas:490-505).
fn chapter_exists(path: &Path, format: OutputFormat) -> bool {
    let extension = match format {
        OutputFormat::Folder => return path.is_dir(),
        OutputFormat::Zip => "zip",
        OutputFormat::Cbz => "cbz",
        OutputFormat::Pdf => "pdf",
        OutputFormat::Epub => "epub",
    };
    let mut file = path.as_os_str().to_owned();
    file.push(".");
    file.push(extension);
    Path::new(&file).is_file() && !path.is_dir()
}

/// The inbox item listing `found`, worded as FMD2's dialog: its caption as the title, its label
/// and memo as the body (`RS_*`, baseunits/uFavoritesManager.pas:176-181).
fn inbox_item(found: &[FavoriteFound], mode: CheckMode) -> NewEvent {
    let chapters: usize = found.iter().map(|f| f.chapters.len()).sum();
    let (kind, title, label, line) = match mode {
        CheckMode::New => (
            "new_chapters",
            "Found new chapter(s)",
            format!(
                "Found {chapters} new chapter from {} manga(s):",
                found.len()
            ),
            "new",
        ),
        CheckMode::Missing => (
            "missing_chapters",
            "Found missing chapter(s)",
            format!(
                "Found {chapters} missing chapter(s) from {} manga(s):",
                found.len()
            ),
            "missing",
        ),
    };
    let mut body = label;
    for f in found {
        body.push_str(&format!(
            "\n- {} <{}> has {} {line} chapter(s).",
            f.title,
            f.website,
            f.chapters.len()
        ));
    }
    NewEvent {
        kind: kind.into(),
        severity: EventSeverity::Info,
        module_id: None,
        task_id: None,
        title: title.into(),
        body: serde_json::Value::String(body),
    }
}

impl Job for FavoritesChecker {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "Favorites check"
    }

    fn status(&self) -> JobStatus {
        lock(&self.inner.status).clone()
    }

    /// Checks every favorite for new chapters.
    fn run(&self) -> Result<(), JobError> {
        self.start(CheckScope::All, CheckMode::New)
            .map_err(|e| match e {
                CheckError::AlreadyRunning => JobError::AlreadyRunning,
                e => JobError::Failed(e.to_string()),
            })
    }

    /// `StopChekForNewChapter` (baseunits/uFavoritesManager.pas:930-952): favorites being
    /// checked finish, the others are skipped, and nothing is reported.
    fn cancel(&self) -> Result<(), JobError> {
        if self.status().phase != JobPhase::Running {
            return Err(JobError::NotRunning);
        }
        self.inner.cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // The status stays consistent even if a holder panicked.
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `at` in Unix milliseconds.
fn unix_ms(at: Instant) -> i64 {
    let wall = SystemTime::now() + at.saturating_duration_since(Instant::now());
    wall.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Runs store work on the blocking thread pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
) -> Result<T, CheckError> {
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => Ok(result?),
        Err(e) => Err(CheckError::Join(e.to_string())),
    }
}
