//! The library's new-chapter check, mirroring FMD2's `TFavoriteManager`, `TFavoriteTask` and
//! `TFavoriteThread` (baseunits/uFavoritesManager.pas:302-1178).

use std::collections::{BTreeSet, HashSet};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fmd_lua::WorkerPool;
use fmd_store::{
    AppDb, Event, EventSeverity, Favorite, FavoriteId, FavoriteInfo, GroupRule, LibraryGroup,
    NewEvent, StoreError,
};
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
use crate::info::{InfoError, InfoOptions, MangaInfo, RateLimits, get_info, split_genres};
use crate::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use crate::metadata::{MetadataJobs, format_of};
use crate::settings::{OutputFormat, SaveToSettings, Settings, SettingsService};

/// `MangaInfo_StatusCompleted` (baseunits/uBaseUnit.pas:230).
const STATUS_COMPLETED: &str = "0";

#[derive(Clone)]
pub struct CheckerConfig {
    pub db: AppDb,
    pub pool: Arc<WorkerPool>,
    pub modules: Arc<ModuleLookup>,
    pub settings: Arc<SettingsService>,
    /// Used when `favorites.auto_download` is on; without one, finds go to the inbox.
    pub queue: Option<Arc<dyn TaskQueue>>,
    pub jobs: JobRegistry,
    /// Websites paused after a rate-limited answer, shared with the other callers of `GetInfo`.
    pub rate_limits: Arc<RateLimits>,
}

pub type QueueFuture<'a> = Pin<Box<dyn Future<Output = Result<TaskId, EngineError>> + Send + 'a>>;

/// Where found chapters are queued (the [`DownloadManager`]).
pub trait TaskQueue: Send + Sync + 'static {
    fn add_task(&self, download: NewDownload) -> QueueFuture<'_>;
}

impl TaskQueue for DownloadManager {
    fn add_task(&self, download: NewDownload) -> QueueFuture<'_> {
        Box::pin(DownloadManager::add_task(self, download))
    }
}

/// A new favorite's directory: `dir`, else `website_dir` (`OverrideSaveTo`), else the default
/// destination, plus the manga folder when generated and not already part of it
/// (`btAddToFavoritesClick`, mangadownloader/forms/frmMain.pas:2804-2827).
pub fn favorite_save_to(
    saveto: &SaveToSettings,
    website: &str,
    website_dir: &str,
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
    save_to(saveto, website, website_dir, &download)
}

/// What a favorite keeps of the website's info: what MangaBaka matching reads, and its genres.
pub fn website_info(info: &MangaInfo) -> FavoriteInfo {
    FavoriteInfo {
        title: info.title.clone(),
        alt_titles: info.alt_titles.clone(),
        authors: info.authors.clone(),
        artists: info.artists.clone(),
        genres: info.genres.clone(),
    }
}

/// A library series' tags and format, as auto groups read them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagsAndFormat {
    /// The website's genres, then those of the accepted MangaBaka match; trimmed, and without
    /// duplicates ignoring case (the first spelling stays).
    pub tags: Vec<String>,
    /// The accepted match's type as [`format_of`] gives it; `None` without a match.
    pub format: Option<String>,
}

/// `favorite`'s tags and format from its `website_genres` (FMD2's comma-separated list) and its
/// accepted MangaBaka match, if `metadata` has one. Blocking.
pub fn tags_and_format(
    metadata: Option<&MetadataJobs>,
    favorite: &Favorite,
    website_genres: &str,
) -> TagsAndFormat {
    let found = metadata.and_then(|m| m.series_match(&favorite.module_id, &favorite.link));
    let mut seen = HashSet::new();
    let tags = split_genres(website_genres)
        .into_iter()
        .chain(found.iter().flat_map(|m| {
            m.series
                .genres
                .iter()
                .map(|g| g.trim().to_owned())
                .filter(|g| !g.is_empty())
        }))
        .filter(|g| seen.insert(g.to_lowercase()))
        .collect();
    TagsAndFormat {
        tags,
        format: found.map(|m| format_of(&m.series.kind).to_owned()),
    }
}

/// `favorite`'s tags and format, from the website genres stored with it. Blocking.
pub fn stored_tags_and_format(
    db: &AppDb,
    metadata: Option<&MetadataJobs>,
    favorite: &Favorite,
) -> Result<TagsAndFormat, StoreError> {
    let genres = db
        .favorites()
        .info(favorite.id)?
        .map(|info| info.genres)
        .unwrap_or_default();
    Ok(tags_and_format(metadata, favorite, &genres))
}

/// Whether a series with `tagged` belongs in a group with `rule`: a tag whole and ignoring case,
/// a format exactly. A series without a MangaBaka match has no format.
fn rule_matches(rule: &GroupRule, tagged: &TagsAndFormat) -> bool {
    match rule {
        GroupRule::Tag(tag) => {
            let tag = tag.trim().to_lowercase();
            tagged.tags.iter().any(|t| t.to_lowercase() == tag)
        }
        GroupRule::Format(format) => tagged.format.as_deref() == Some(format.as_str()),
    }
}

/// Puts each of `ids` that is in no group in the first group, by order, whose rule matches its
/// tags and format. A series in a group, by hand or by rule, stays. Returns how many it moved.
/// Blocking.
pub fn auto_group(
    db: &AppDb,
    metadata: Option<&MetadataJobs>,
    ids: &[FavoriteId],
) -> Result<usize, StoreError> {
    let groups: Vec<LibraryGroup> = db
        .library_groups()
        .list()?
        .into_iter()
        .filter(|g| g.rule.is_some())
        .collect();
    if groups.is_empty() {
        return Ok(0);
    }
    let mut moved = 0;
    for id in ids {
        let Some(favorite) = db.favorites().get(*id)?.filter(|f| f.group_id.is_none()) else {
            continue;
        };
        let tagged = stored_tags_and_format(db, metadata, &favorite)?;
        let group = groups
            .iter()
            .find(|g| g.rule.as_ref().is_some_and(|r| rule_matches(r, &tagged)));
        if let Some(group) = group
            && db.favorites().group_if_ungrouped(favorite.id, group.id)?
        {
            moved += 1;
        }
    }
    Ok(moved)
}

/// [`auto_group`] for every series in the library. Blocking.
pub fn auto_group_library(
    db: &AppDb,
    metadata: Option<&MetadataJobs>,
) -> Result<usize, StoreError> {
    let ids: Vec<FavoriteId> = db
        .favorites()
        .list()?
        .into_iter()
        .filter(|f| f.group_id.is_none())
        .map(|f| f.id)
        .collect();
    auto_group(db, metadata, &ids)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckScope {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundChapter {
    pub name: String,
    /// Relative to the module's `RootURL`.
    pub link: String,
    /// 1-based position in the series' chapter list, FMD2's `%NUMBERING%`.
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteFound {
    pub favorite: FavoriteId,
    pub title: String,
    pub module_id: String,
    /// The module's name, FMD2's `Website`.
    pub website: String,
    pub link: String,
    pub save_to: String,
    /// For the chapter names.
    pub authors: String,
    pub artists: String,
    pub chapters: Vec<FoundChapter>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckReport {
    pub checked: u64,
    /// Favorites with new (or missing) chapters.
    pub found: Vec<FavoriteFound>,
    /// Completed series with nothing new; removed when `favorites.remove_completed` is on.
    pub completed: Vec<FavoriteId>,
    pub queued: Vec<TaskId>,
    /// Series whose website could not be fetched, or was paused after a rate-limited answer.
    pub failed: Vec<FailedSeries>,
    /// The run's inbox item: found chapters reported rather than queued, or a missing-chapter
    /// check's result.
    pub inbox: Option<Event>,
    pub cancelled: bool,
}

/// A series a check could not check, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedSeries {
    pub title: String,
    /// The module's name, FMD2's `Website`.
    pub website: String,
    pub reason: String,
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

#[derive(Debug, Clone, PartialEq)]
pub enum CheckerEvent {
    Job(FavoritesEvent),
    Inbox(Event),
}

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

type InfoHook = dyn Fn(&str) + Send + Sync;

/// Checks favorites for new chapters (`TFavoriteManager`) as the `favorites` background job.
/// Clones share the runs.
#[derive(Clone)]
pub struct FavoritesChecker {
    inner: Arc<Inner>,
}

struct Inner {
    config: CheckerConfig,
    on_event: Box<dyn Fn(CheckerEvent) + Send + Sync>,
    status: Mutex<JobStatus>,
    /// `Terminated`.
    cancelled: AtomicBool,
    /// Counts the runs that ended, so a scheduled check can wait for a manual one.
    ended: watch::Sender<u64>,
    on_info_changed: OnceLock<Box<InfoHook>>,
}

struct CompletedSeries {
    id: FavoriteId,
    title: String,
    website: String,
}

enum Checked {
    Found(FavoriteFound),
    Completed(CompletedSeries),
    Nothing,
    Failed(FailedSeries),
}

impl FavoritesChecker {
    pub const ID: &str = "favorites";

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
                on_info_changed: OnceLock::new(),
            }),
        }
    }

    /// Calls `hook` at the end of a run with each module whose favorites' website info (title,
    /// alt titles, authors, artists) changed, so they can be matched again. It runs on the async
    /// runtime: hand blocking work off. Set once; later hooks are ignored.
    pub fn on_info_changed(&self, hook: impl Fn(&str) + Send + Sync + 'static) {
        let _ = self.inner.on_info_changed.set(Box::new(hook));
    }

    /// Starts a background run unless one is going (`CheckForNewChapter`/
    /// `CheckForMissingChapters`, baseunits/uFavoritesManager.pas:832-928).
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

    /// Like [`Self::start`], but waits for the run to end.
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

    /// Runs FMD2's timed checks, following setting changes: at startup (`tmStartupTimer`,
    /// mangadownloader/forms/frmMain.pas:2078-2082), then every interval after the last ended
    /// (`tmCheckFavorites`, :1871-1881, :6335, re-armed at baseunits/uFavoritesManager.pas:597).
    /// A check already going counts as the due one, so runs never overlap. Never returns.
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

    /// A scheduled check (`isAuto`); a check already going counts as this one.
    async fn scheduled_check(&self) {
        let mut ended = self.inner.ended.subscribe();
        ended.mark_unchanged();
        // A failed run has been logged.
        if let Err(CheckError::AlreadyRunning) = self.check(CheckScope::All, CheckMode::New).await {
            // The sender lives in `self`, so this only returns once a run ended.
            let _ = ended.changed().await;
        }
    }

    /// Unix milliseconds.
    fn set_next_run(&self, at: Option<i64>) {
        self.update(|status| status.next_run = at);
    }

    /// `isRunning`.
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

    /// `TFavoriteTask.Execute` (baseunits/uFavoritesManager.pas:607-667), on at most
    /// `connections.max_favorite_threads` at once (`GetNext`, :711-742).
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
        let skipped: Arc<Mutex<HashSet<String>>> = Arc::default();
        let info_changed: Arc<Mutex<BTreeSet<String>>> = Arc::default();
        let mut running = JoinSet::new();
        // To name a series whose check failed.
        let names: Vec<(String, String)> = favorites
            .iter()
            .map(|f| (f.title.clone(), self.website(&f.module_id)))
            .collect();
        for (index, favorite) in favorites.into_iter().enumerate() {
            let permit = permits.clone().acquire_owned().await;
            let this = self.clone();
            let skipped = skipped.clone();
            let info_changed = info_changed.clone();
            running.spawn(async move {
                let _permit = permit;
                if this.inner.cancelled.load(Ordering::SeqCst) {
                    return (index, Ok(None));
                }
                let checked = this
                    .check_one(&favorite, mode, &skipped, &info_changed)
                    .await;
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
        if let Some(hook) = self.inner.on_info_changed.get() {
            for module_id in lock(&info_changed).iter() {
                hook(module_id);
            }
        }
        // In library order, as FMD2 lists them.
        results.sort_by_key(|(index, _)| *index);
        let mut report = CheckReport::default();
        let mut completed = Vec::new();
        for (index, result) in results {
            let checked = match result {
                Ok(Some(checked)) => checked,
                Ok(None) => continue,
                // Per-favorite `ExceptionHandle` (baseunits/uFavoritesManager.pas:391-394).
                Err(e) => {
                    tracing::warn!(target: "fmd_core", "favorites check: {e}");
                    if let Some((title, website)) = names.get(index) {
                        report.failed.push(FailedSeries {
                            title: title.clone(),
                            website: website.clone(),
                            reason: e.to_string(),
                        });
                    }
                    continue;
                }
            };
            if !matches!(checked, Checked::Failed(_)) {
                report.checked += 1;
            }
            match checked {
                Checked::Found(found) => report.found.push(found),
                Checked::Completed(series) => {
                    report.completed.push(series.id);
                    completed.push(series);
                }
                Checked::Failed(failed) => report.failed.push(failed),
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

    /// `DoCheck`/`DoCheckMissing` (baseunits/uFavoritesManager.pas:329-531). Beyond FMD2, a
    /// favorite of a website paused after a rate-limited answer is skipped; the first skip of
    /// each website in a run (`skipped`) logs. A favorite whose website info changed adds its
    /// module to `info_changed`.
    async fn check_one(
        &self,
        favorite: &Favorite,
        mode: CheckMode,
        skipped: &Mutex<HashSet<String>>,
        info_changed: &Mutex<BTreeSet<String>>,
    ) -> Result<Option<Checked>, CheckError> {
        let rate_limits = &self.inner.config.rate_limits;
        if let Some(e) = rate_limits.paused(&favorite.module_id) {
            if lock(skipped).insert(favorite.module_id.clone()) {
                tracing::warn!(
                    target: "fmd_core",
                    "skipping the favorites of {}: it is limiting requests",
                    self.website(&favorite.module_id)
                );
            }
            return Ok(Some(self.failed(favorite, &e)));
        }
        let info = match self.info(favorite).await {
            Ok(info) => info,
            Err(e) => {
                rate_limits.pause_if_limited(&favorite.module_id, &e);
                tracing::warn!(target: "fmd_core", "checking favorite {}: {e}", favorite.title);
                if !matches!(e, InfoError::RateLimited { .. }) {
                    self.record_failure(favorite.id, e.to_string()).await?;
                }
                return Ok(Some(self.failed(favorite, &e)));
            }
        };
        let chapters = match mode {
            CheckMode::New => self.new_chapters(favorite, &info).await?,
            CheckMode::Missing => self.missing_chapters(favorite, &info).await?,
        };
        if self.inner.cancelled.load(Ordering::SeqCst) {
            return Ok(None);
        }
        // Re-read: it may have been edited or removed during the check.
        let Some((favorite, changed)) = self
            .store_checked(favorite.id, &info, !chapters.is_empty())
            .await?
        else {
            return Ok(None);
        };
        if changed {
            lock(info_changed).insert(favorite.module_id.clone());
        }
        if mode == CheckMode::Missing {
            self.unmark(&favorite, &chapters).await?;
        }
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

    fn failed(&self, favorite: &Favorite, e: &InfoError) -> Checked {
        Checked::Failed(FailedSeries {
            title: favorite.title.clone(),
            website: self.website(&favorite.module_id),
            reason: e.to_string(),
        })
    }

    /// The module's name, or its ID when gone.
    fn website(&self, module_id: &str) -> String {
        (self.inner.config.modules)(module_id)
            .map_or_else(|| module_id.to_owned(), |m| m.def().name)
    }

    /// Removes and reports `completed` in place of FMD2's confirmation dialog (`ShowResult`,
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

    async fn notify(&self, event: NewEvent) -> Result<Event, CheckError> {
        let db = self.inner.config.db.clone();
        let stored = blocking(move || db.events().push(&event)).await?;
        (self.inner.on_event)(CheckerEvent::Inbox(stored.clone()));
        Ok(stored)
    }

    /// Queues the finds, or reports them in place of FMD2's new-chapter dialog (`ShowResult`,
    /// baseunits/uFavoritesManager.pas:1047-1073).
    async fn show_result(
        &self,
        report: &mut CheckReport,
        mode: CheckMode,
    ) -> Result<(), CheckError> {
        if report.found.is_empty() {
            if mode == CheckMode::Missing {
                let mut item = nothing_missing(report);
                add_failures(&mut item, "checked", &report.failed);
                report.inbox = Some(self.notify(item).await?);
            }
            return Ok(());
        }
        let auto_download = self.inner.config.settings.get().favorites.auto_download;
        let mut item = match &self.inner.config.queue {
            Some(queue) if auto_download => {
                let mut queued = Vec::new();
                let mut not_queued = Vec::new();
                for found in &report.found {
                    match self.queue(queue.as_ref(), found).await? {
                        Ok(task) => {
                            report.queued.push(task);
                            queued.push(found.clone());
                        }
                        Err(e) => not_queued.push(FailedSeries {
                            title: found.title.clone(),
                            website: found.website.clone(),
                            reason: e.to_string(),
                        }),
                    }
                }
                // Beyond FMD2, which queues missing chapters without a word (:1050-1051).
                if mode == CheckMode::New {
                    return Ok(());
                }
                let mut item = if queued.is_empty() {
                    inbox_item(&report.found, mode, Reported::Found)
                } else {
                    inbox_item(&queued, mode, Reported::Queued)
                };
                add_failures(&mut item, "queued", &not_queued);
                item
            }
            _ => inbox_item(&report.found, mode, Reported::Found),
        };
        if mode == CheckMode::Missing {
            add_failures(&mut item, "checked", &report.failed);
        }
        report.inbox = Some(self.notify(item).await?);
        Ok(())
    }

    /// Queues `found` and marks it downloaded (`ShowResult`,
    /// baseunits/uFavoritesManager.pas:1087-1129). A failure is logged and returned; the
    /// chapters stay new.
    async fn queue(
        &self,
        queue: &dyn TaskQueue,
        found: &FavoriteFound,
    ) -> Result<Result<TaskId, EngineError>, CheckError> {
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
                return Ok(Err(e));
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
        Ok(Ok(task))
    }

    /// Stores chapter count and status (`DoCheck`, baseunits/uFavoritesManager.pas:351-353),
    /// check time, and update time when chapters were found (:373-378); beyond FMD2, also the
    /// chapter links for the new-chapter badge and a non-empty cover link for the library, and
    /// forgets earlier failed checks. `None` when the favorite was removed meanwhile.
    async fn store_checked(
        &self,
        id: FavoriteId,
        info: &MangaInfo,
        found: bool,
    ) -> Result<Option<(Favorite, bool)>, CheckError> {
        let now = now_ms();
        let chapters = u32::try_from(info.chapters.len()).unwrap_or(u32::MAX);
        let status = info.status.clone();
        let links: Vec<String> = info.chapters.iter().map(|c| c.link.clone()).collect();
        let cover = Some(info.cover_link.clone()).filter(|c| !c.is_empty());
        let website_info = website_info(info);
        let db = self.inner.config.db.clone();
        // Not atomic: it only races with another write to this favorite, which a PATCH redoes.
        blocking(move || {
            db.favorites().clear_check_failures(id)?;
            let Some(mut favorite) = db.favorites().get(id)? else {
                return Ok(None);
            };
            favorite.current_chapter = chapters;
            favorite.status = status;
            favorite.date_last_checked = Some(now);
            if cover.is_some() {
                favorite.cover_url = cover;
            }
            if found {
                favorite.date_last_updated = Some(now);
            }
            db.favorites().update(&favorite)?;
            let links: Vec<&str> = links.iter().map(String::as_str).collect();
            db.favorites().set_chapter_links(id, &links)?;
            let changed = db.favorites().set_info(id, &website_info)?;
            Ok(Some((favorite, changed)))
        })
        .await
    }

    /// Beyond FMD2, which keeps the mark, clears the downloaded mark of missing `chapters`, so
    /// the new-chapter badge counts them and the next new-chapter check finds them again.
    async fn unmark(
        &self,
        favorite: &Favorite,
        chapters: &[FoundChapter],
    ) -> Result<(), CheckError> {
        if chapters.is_empty() {
            return Ok(());
        }
        let db = self.inner.config.db.clone();
        let (module, link) = (favorite.module_id.clone(), favorite.link.clone());
        let chapters: Vec<String> = chapters.iter().map(|c| c.link.clone()).collect();
        blocking(move || {
            let links: Vec<&str> = chapters.iter().map(String::as_str).collect();
            db.downloaded_chapters().unmark(&module, &link, &links)
        })
        .await
    }

    /// Beyond FMD2, which only logs a failed check (baseunits/uFavoritesManager.pas:391-394),
    /// counts it so the library can flag a series whose website keeps failing.
    async fn record_failure(&self, id: FavoriteId, error: String) -> Result<(), CheckError> {
        let db = self.inner.config.db.clone();
        let now = now_ms();
        blocking(move || db.favorites().record_check_failure(id, &error, now)).await
    }

    /// Beyond FMD2, a run over all favorites skips one that has [`FAILING_CHECKS`] or more failed
    /// checks until [`FAILING_RETRY`] after the last.
    async fn favorites(&self, scope: CheckScope) -> Result<Vec<Favorite>, CheckError> {
        let db = self.inner.config.db.clone();
        let all = blocking(move || db.favorites().list()).await?;
        let now = now_ms();
        Ok(all
            .into_iter()
            .filter(|f| f.enabled && !f.link.trim().is_empty())
            .filter(|f| match &scope {
                CheckScope::All => !backing_off(f, now),
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

    /// Chapters not in the downloaded list, compared case-insensitively (`DoCheck`,
    /// baseunits/uFavoritesManager.pas:357-371).
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

    /// Chapters with no file or folder in the favorite's directory, looked for as the format
    /// the directory holds most of, else the output format (`DoCheckMissing`,
    /// baseunits/uFavoritesManager.pas:425-511).
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
        let (title, dir) = (favorite.title.clone(), favorite.save_to.clone());
        let chapters = info.chapters.len();
        let series = info.clone();
        let on_disk = match tokio::task::spawn_blocking(move || {
            chapters_on_disk(&settings, &website, &title, &series, Path::new(&dir))
        })
        .await
        {
            Ok(on_disk) => on_disk,
            Err(e) => return Err(CheckError::Join(e.to_string())),
        };
        // An unreadable folder finds no chapter file, so all are missing, as in FMD2 (:430-505).
        let missing = on_disk.map_or_else(
            |_| vec![true; chapters],
            |on_disk| on_disk.into_iter().map(|on| !on).collect(),
        );
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

/// Consecutive failed checks after which a series counts as failing: the library flags it and
/// runs over all favorites check it at most every [`FAILING_RETRY`].
pub const FAILING_CHECKS: u32 = 3;

/// How long runs over all favorites leave a failing series alone after its last failed check.
pub const FAILING_RETRY: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether `favorite` is failing and failed within [`FAILING_RETRY`] of `now` (Unix ms).
fn backing_off(favorite: &Favorite, now: i64) -> bool {
    let retry = i64::try_from(FAILING_RETRY.as_millis()).unwrap_or(i64::MAX);
    favorite.check_failures >= FAILING_CHECKS
        && favorite
            .date_last_failed
            .is_some_and(|at| now.saturating_sub(at) < retry)
}

fn found_chapter(index: usize, chapter: &crate::info::Chapter) -> FoundChapter {
    FoundChapter {
        name: chapter.name.clone(),
        link: chapter.link.clone(),
        number: u32::try_from(index + 1).unwrap_or(u32::MAX),
    }
}

/// Whether each of `info.chapters` has its file or folder in `dir`, a series' `save_to`; named by
/// the chapter rename template, with `website` and `manga` as the website's and series' names,
/// and looked for as the format `dir` holds most of, else the output format (`DoCheckMissing`,
/// baseunits/uFavoritesManager.pas:425-511). A `dir` that doesn't exist holds no chapter; any
/// other error reading it is returned. Blocking.
pub fn chapters_on_disk(
    settings: &Settings,
    website: &str,
    manga: &str,
    info: &MangaInfo,
    dir: &Path,
) -> std::io::Result<Vec<bool>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(vec![false; info.chapters.len()]);
        }
        Err(e) => return Err(e),
    };
    let format = detect_format(entries).unwrap_or(settings.output.format);
    let options = rename_options(&settings.saveto);
    Ok(info
        .chapters
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let numbering = format!("{:04}", i + 1);
            let ctx = RenameContext {
                website,
                manga,
                author: &info.authors,
                artist: &info.artists,
                chapter: &c.name,
                numbering: &numbering,
                filename: "",
            };
            let name = custom_rename(&settings.saveto.chapter_rename, &ctx, &options);
            chapter_exists(&dir.join(name), format)
        })
        .collect())
}

/// The chapter format a folder with `entries` holds most of; ties go to the earlier of folder,
/// `.cbz`, `.zip`, `.pdf`, `.epub` (`DoCheckMissing`, baseunits/uFavoritesManager.pas:430-473).
fn detect_format(entries: std::fs::ReadDir) -> Option<OutputFormat> {
    let (mut dirs, mut cbz, mut zip, mut pdf, mut epub) = (0, 0, 0, 0, 0);
    for entry in entries.flatten() {
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

/// `path` is without extension. A folder next to an archive means packing was interrupted, so
/// it counts as missing (baseunits/uFavoritesManager.pas:490-505).
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

/// Whether an inbox item's chapters were found, or found and queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reported {
    Found,
    /// Beyond FMD2, which queues without a word (baseunits/uFavoritesManager.pas:1050-1051).
    Queued,
}

/// Worded as FMD2's dialog (`RS_*`, baseunits/uFavoritesManager.pas:176-181).
fn inbox_item(found: &[FavoriteFound], mode: CheckMode, reported: Reported) -> NewEvent {
    let chapters: usize = found.iter().map(|f| f.chapters.len()).sum();
    let (kind, title, label, line) = match (mode, reported) {
        (CheckMode::New, _) => (
            "new_chapters",
            "Found new chapter(s)".to_owned(),
            format!(
                "Found {chapters} new chapter from {} manga(s):",
                found.len()
            ),
            "new",
        ),
        (CheckMode::Missing, Reported::Found) => (
            "missing_chapters",
            "Found missing chapter(s)".to_owned(),
            format!(
                "Found {chapters} missing chapter(s) from {} manga(s):",
                found.len()
            ),
            "missing",
        ),
        (CheckMode::Missing, Reported::Queued) => (
            "missing_chapters",
            format!("Queued {chapters} missing chapter(s)"),
            format!(
                "Queued {chapters} missing chapter(s) from {} manga(s):",
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
        title,
        body: serde_json::Value::String(body),
    }
}

/// Beyond FMD2, which only logs a series it could not check (`ExceptionHandle`,
/// baseunits/uFavoritesManager.pas:391-394): a line for each in `item`, which turns into a
/// warning. `step` is what could not be done, e.g. `checked`.
fn add_failures(item: &mut NewEvent, step: &str, failed: &[FailedSeries]) {
    if failed.is_empty() {
        return;
    }
    let mut lines: Vec<String> = item
        .body
        .as_str()
        .filter(|b| !b.is_empty())
        .map(str::to_owned)
        .into_iter()
        .collect();
    lines.extend(failed.iter().map(|f| {
        format!(
            "- {} <{}>: could not be {step}: {}",
            f.title, f.website, f.reason
        )
    }));
    let body = lines.join("\n");
    item.body = serde_json::Value::String(body);
    item.severity = EventSeverity::Warning;
}

/// The kind of a missing-chapter check's item that found nothing: not `missing_chapters`, so
/// notifications don't announce it as found chapters.
const NOTHING_MISSING_KIND: &str = "missing_check";

/// Beyond FMD2, whose missing-chapter check stays silent when nothing is missing
/// (baseunits/uFavoritesManager.pas:1048). When no series could be checked, the failures alone
/// make the body.
fn nothing_missing(report: &CheckReport) -> NewEvent {
    let (title, body) = if report.checked == 0 && !report.failed.is_empty() {
        ("Could not check for missing chapters", String::new())
    } else {
        (
            "No missing chapters",
            format!(
                "Checked {} series; every chapter is on disk.",
                report.checked
            ),
        )
    };
    NewEvent {
        kind: NOTHING_MISSING_KIND.into(),
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

fn unix_ms(at: Instant) -> i64 {
    let wall = SystemTime::now() + at.saturating_duration_since(Instant::now());
    wall.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
) -> Result<T, CheckError> {
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => Ok(result?),
        Err(e) => Err(CheckError::Join(e.to_string())),
    }
}
