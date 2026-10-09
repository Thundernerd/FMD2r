//! The MangaBaka database as a background job: downloading and building it, then matching every
//! list against it, reported like the list jobs. Also matches a list after it changed, and
//! refreshes the database on its schedule (`metadata.mangabaka.refresh_days`).

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use fmd_http::TerminateToken;
use fmd_store::ListsDb;
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use super::db::now_ms;
use super::{
    BuildProgress, DbInfo, ListModule, MangaBakaDb, MatchScope, Matcher, Metadata, MetadataError,
};
use crate::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use crate::settings::SettingsService;

/// How often the schedule checks whether the database is due for a refresh.
const SCHEDULE_TICK: Duration = Duration::from_secs(60 * 60);
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// What happened to the database job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetadataEventKind {
    Started,
    Progress,
    Finished,
    Cancelled,
    Failed,
}

/// The step the database job is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetadataPhase {
    /// Downloading the dump and building `metadata.db` from it; `done` and `total` count bytes.
    Downloading,
    /// Matching the lists against the new database; `done` and `total` count lists.
    Matching,
}

/// One step of the database job, for the Settings page (`job.metadata.<kind>` events).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct MetadataEvent {
    pub kind: MetadataEventKind,
    pub phase: MetadataPhase,
    pub status_text: String,
    /// Work items of the phase done.
    pub done: u64,
    /// Work items of the phase; 0 when unknown.
    pub total: u64,
    /// Why it failed.
    pub error: Option<String>,
}

/// Why the database job could not do what was asked.
#[derive(Debug, Error)]
pub enum MetadataJobError {
    #[error("the MangaBaka database is already being downloaded")]
    AlreadyRunning,
    #[error("the MangaBaka database is not being downloaded")]
    NotRunning,
    #[error("starting the job thread: {0}")]
    Spawn(#[from] std::io::Error),
    #[error(transparent)]
    Metadata(#[from] MetadataError),
}

/// The `RootURL` of a loaded module, which its list's links are relative to.
pub trait ModuleRoots: Send + Sync + 'static {
    fn root_url(&self, module_id: &str) -> Option<String>;
}

impl<F> ModuleRoots for F
where
    F: Fn(&str) -> Option<String> + Send + Sync + 'static,
{
    fn root_url(&self, module_id: &str) -> Option<String> {
        self(module_id)
    }
}

type EventSink = dyn Fn(MetadataEvent) + Send + Sync;

/// Downloads, refreshes and removes the MangaBaka database, and matches lists against it.
/// Cheap to clone.
#[derive(Clone)]
pub struct MetadataJobs {
    inner: Arc<Inner>,
}

struct Inner {
    db: Arc<MangaBakaDb>,
    matcher: Matcher,
    lists: ListsDb,
    modules: Box<dyn ModuleRoots>,
    settings: Arc<SettingsService>,
    /// The running download's token.
    running: Mutex<Option<TerminateToken>>,
    status: Mutex<JobStatus>,
    /// When the last download failed; the schedule waits a day before trying again.
    failed_at: Mutex<Option<i64>>,
    /// The last event of the running download, for a page opened while it runs.
    last_event: Mutex<Option<MetadataEvent>>,
    /// Held while matching, so a list update and a refresh do not match the same list at once.
    matching: Mutex<()>,
    registry: OnceLock<JobRegistry>,
    on_event: Box<EventSink>,
}

impl MetadataJobs {
    /// The job's id in `/api/jobs/{id}`.
    pub const ID: &str = "mangabaka";

    /// Jobs on `db`, matching `lists` with `matcher` (the modules' root URLs from `modules`),
    /// refreshing as `settings` say, and passing every event to `on_event`.
    pub fn new(
        db: Arc<MangaBakaDb>,
        matcher: Matcher,
        lists: ListsDb,
        modules: impl ModuleRoots,
        settings: Arc<SettingsService>,
        on_event: impl Fn(MetadataEvent) + Send + Sync + 'static,
    ) -> Self {
        let jobs = Self {
            inner: Arc::new(Inner {
                db,
                matcher,
                lists,
                modules: Box::new(modules),
                settings,
                running: Mutex::default(),
                status: Mutex::new(JobStatus {
                    phase: JobPhase::Idle,
                    done: 0,
                    total: 0,
                    last_run: None,
                    next_run: None,
                    last_error: None,
                }),
                failed_at: Mutex::default(),
                last_event: Mutex::default(),
                matching: Mutex::default(),
                registry: OnceLock::new(),
                on_event: Box::new(on_event),
            }),
        };
        jobs.inner.status().next_run = jobs.inner.next_refresh();
        jobs
    }

    /// The database, when one is downloaded.
    pub fn current(&self) -> Option<Arc<Metadata>> {
        self.inner.db.current()
    }

    /// When the database was built and how large it is; `None` when there is none.
    pub fn info(&self) -> Option<DbInfo> {
        self.inner.db.info()
    }

    /// Whether a download is running.
    pub fn is_running(&self) -> bool {
        lock(&self.inner.running).is_some()
    }

    /// The last event of the running download.
    pub fn progress(&self) -> Option<MetadataEvent> {
        if self.is_running() {
            lock(&self.inner.last_event).clone()
        } else {
            None
        }
    }

    /// Starts downloading (or refreshing) the database on a thread of its own, then matching
    /// every list against it.
    pub fn download(&self) -> Result<(), MetadataJobError> {
        let terminate = TerminateToken::new();
        {
            let mut running = lock(&self.inner.running);
            if running.is_some() {
                return Err(MetadataJobError::AlreadyRunning);
            }
            *running = Some(terminate.clone());
            let mut status = self.inner.status();
            status.phase = JobPhase::Running;
            status.done = 0;
            status.total = 0;
            status.last_error = None;
            status.last_run = Some(now_ms());
        }
        self.inner.changed();
        let inner = self.inner.clone();
        let spawned = std::thread::Builder::new()
            .name("fmd-mangabaka".into())
            .spawn(move || inner.run(&terminate));
        if let Err(e) = spawned {
            self.inner.finished(Some(e.to_string()));
            return Err(e.into());
        }
        Ok(())
    }

    /// Asks the running download to stop. The database it would have replaced stays.
    pub fn cancel(&self) -> Result<(), MetadataJobError> {
        match lock(&self.inner.running).as_ref() {
            Some(token) => {
                token.terminate();
                Ok(())
            }
            None => Err(MetadataJobError::NotRunning),
        }
    }

    /// Deletes the database and every list title's match in it.
    pub fn remove(&self) -> Result<(), MetadataJobError> {
        if self.is_running() {
            return Err(MetadataJobError::AlreadyRunning);
        }
        let _matching = lock(&self.inner.matching);
        self.inner.db.remove()?;
        self.inner
            .lists
            .matches()
            .clear()
            .map_err(MetadataError::Lists)?;
        self.inner.status().next_run = None;
        self.inner.changed();
        Ok(())
    }

    /// Matches the titles of `module_id`'s list that are new or changed, after a list update or
    /// FMD2-DB import. Does nothing without a database. Blocks.
    pub fn list_changed(&self, module_id: &str, terminate: &TerminateToken) {
        let Some(meta) = self.inner.db.current() else {
            return;
        };
        let Some(root_url) = self.inner.modules.root_url(module_id) else {
            return;
        };
        let module = ListModule {
            id: module_id.to_owned(),
            root_url,
        };
        let _matching = lock(&self.inner.matching);
        match self
            .inner
            .matcher
            .match_module(&meta, &module, MatchScope::Changed, terminate)
        {
            Ok(report) => tracing::info!(target: "fmd_core",
                "matched {} titles of {module_id} against MangaBaka ({} accepted)",
                report.examined, report.accepted),
            Err(e) => tracing::warn!(target: "fmd_core",
                "matching {module_id} against MangaBaka: {e}"),
        }
    }

    /// Matches the titles every list still owes a match: new, changed, matched against an older
    /// database (a refresh whose matching was cancelled or failed), or matched while MangaDex
    /// could not be asked. Does nothing without a database. Blocks.
    pub fn catch_up(&self) {
        let summaries = match self.inner.lists.masterlist().summaries() {
            Ok(summaries) => summaries,
            Err(e) => {
                tracing::warn!(target: "fmd_core", "catching up MangaBaka matches: {e}");
                return;
            }
        };
        let terminate = TerminateToken::new();
        for summary in summaries.into_iter().filter(|s| s.count > 0) {
            if self.is_running() {
                return;
            }
            self.list_changed(&summary.module_id, &terminate);
        }
    }

    /// Adds the job to `registry` and announces its changes there.
    pub fn register(&self, registry: &JobRegistry) {
        // Registered once; a second registry is not told about changes.
        let _ = self.inner.registry.set(registry.clone());
        registry.register(self.clone());
        registry.changed(Self::ID);
    }

    /// Refreshes a downloaded database every `metadata.mangabaka.refresh_days` days (never when
    /// 0, nor before the first download; a day after a failed one), and catches up on the matches
    /// lists still owe ([`MetadataJobs::catch_up`]) at startup and every hour. Runs until the
    /// task is dropped.
    pub async fn schedule(self) {
        let mut tick = tokio::time::interval(SCHEDULE_TICK);
        let mut settings = self.inner.settings.subscribe();
        loop {
            let ticked = tokio::select! {
                _ = tick.tick() => true,
                changed = settings.changed() => if changed.is_err() { return } else { false },
            };
            let next = self.inner.next_refresh();
            if self.inner.status().next_run != next {
                self.inner.status().next_run = next;
                self.inner.changed();
            }
            if next.is_some_and(|at| at <= now_ms()) && !self.is_running() {
                let jobs = self.clone();
                // Starting spawns a thread.
                let started = tokio::task::spawn_blocking(move || jobs.download()).await;
                if let Ok(Err(e)) = started {
                    tracing::warn!(target: "fmd_core", "refreshing the MangaBaka database: {e}");
                }
            } else if ticked && !self.is_running() && self.current().is_some() {
                let jobs = self.clone();
                let _ = tokio::task::spawn_blocking(move || jobs.catch_up()).await;
            }
        }
    }
}

fn event(kind: MetadataEventKind, phase: MetadataPhase, status_text: String) -> MetadataEvent {
    MetadataEvent {
        kind,
        phase,
        status_text,
        done: 0,
        total: 0,
        error: None,
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // The guarded values stay consistent even if a holder panicked.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    fn status(&self) -> MutexGuard<'_, JobStatus> {
        lock(&self.status)
    }

    fn changed(&self) {
        if let Some(registry) = self.registry.get() {
            registry.changed(MetadataJobs::ID);
        }
    }

    /// When the downloaded database is due for a refresh; `None` without one, or when automatic
    /// refreshes are off.
    fn next_refresh(&self) -> Option<i64> {
        let days = self.settings.get().metadata.mangabaka.refresh_days;
        if days == 0 {
            return None;
        }
        let built_at = self.db.info()?.built_at;
        let due = built_at.saturating_add(i64::from(days) * DAY_MS);
        let retry = lock(&self.failed_at).map(|at| at.saturating_add(DAY_MS));
        Some(due.max(retry.unwrap_or(due)))
    }

    /// Records `event`'s counts as the job's progress and sends it.
    fn report(&self, event: MetadataEvent) {
        {
            let mut status = self.status();
            status.done = event.done;
            status.total = event.total;
        }
        self.changed();
        self.send(event);
    }

    fn send(&self, event: MetadataEvent) {
        if event.kind == MetadataEventKind::Started || event.kind == MetadataEventKind::Progress {
            *lock(&self.last_event) = Some(event.clone());
        }
        (self.on_event)(event);
    }

    /// The download job's thread.
    fn run(&self, terminate: &TerminateToken) {
        self.send(event(
            MetadataEventKind::Started,
            MetadataPhase::Downloading,
            "Downloading...".into(),
        ));
        let mut last_report: Option<std::time::Instant> = None;
        let built = self.db.refresh(terminate, &mut |p: &BuildProgress| {
            // At most a few events a second.
            if last_report.is_some_and(|at| at.elapsed() < Duration::from_millis(250)) {
                return;
            }
            last_report = Some(std::time::Instant::now());
            let mut event = event(
                MetadataEventKind::Progress,
                MetadataPhase::Downloading,
                format!("Downloading and building... {} series", p.series),
            );
            event.done = p.bytes;
            event.total = p.total_bytes.unwrap_or(0);
            self.report(event);
        });
        let summary = match built {
            Ok(summary) => summary,
            Err(MetadataError::Cancelled) => return self.end_cancelled(MetadataPhase::Downloading),
            Err(e) => return self.end_failed(MetadataPhase::Downloading, &e),
        };
        tracing::info!(target: "fmd_core",
            "MangaBaka database built: {} series, {} merged, {} skipped",
            summary.series, summary.merged, summary.skipped);

        match self.match_all(terminate) {
            Ok(false) => {
                self.finished(None);
                self.send(event(
                    MetadataEventKind::Finished,
                    MetadataPhase::Matching,
                    format!("{} series", summary.series),
                ));
            }
            Ok(true) => self.end_cancelled(MetadataPhase::Matching),
            Err(e) => self.end_failed(MetadataPhase::Matching, &e),
        }
    }

    /// Matches every list with titles against the new database. Returns whether it was
    /// cancelled.
    fn match_all(&self, terminate: &TerminateToken) -> Result<bool, MetadataError> {
        let Some(meta) = self.db.current() else {
            return Ok(false);
        };
        let modules: Vec<ListModule> = self
            .lists
            .masterlist()
            .summaries()
            .map_err(MetadataError::Lists)?
            .into_iter()
            .filter(|s| s.count > 0)
            .filter_map(|s| {
                let root_url = self.modules.root_url(&s.module_id)?;
                Some(ListModule {
                    id: s.module_id,
                    root_url,
                })
            })
            .collect();
        let total = modules.len() as u64;
        let _matching = lock(&self.matching);
        for (done, module) in modules.iter().enumerate() {
            if terminate.is_terminated() {
                return Ok(true);
            }
            let mut event = event(
                MetadataEventKind::Progress,
                MetadataPhase::Matching,
                "Matching the lists...".into(),
            );
            event.done = done as u64;
            event.total = total;
            self.report(event);
            let report = self
                .matcher
                .match_module(&meta, module, MatchScope::All, terminate)?;
            if report.cancelled {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn end_cancelled(&self, phase: MetadataPhase) {
        self.finished(None);
        self.send(event(MetadataEventKind::Cancelled, phase, String::new()));
    }

    fn end_failed(&self, phase: MetadataPhase, error: &MetadataError) {
        tracing::warn!(target: "fmd_core", "MangaBaka database: {error}");
        let message = error.to_string();
        self.finished(Some(message.clone()));
        let mut event = event(MetadataEventKind::Failed, phase, String::new());
        event.error = Some(message);
        self.send(event);
    }

    /// Ends the running download, which failed with `error` when one is given.
    fn finished(&self, error: Option<String>) {
        {
            let mut running = lock(&self.running);
            *running = None;
            *lock(&self.last_event) = None;
            let mut status = self.status();
            status.phase = if error.is_some() {
                JobPhase::Failed
            } else {
                JobPhase::Done
            };
            *lock(&self.failed_at) = error.is_some().then(now_ms);
            status.last_error = error;
            status.next_run = self.next_refresh();
        }
        self.changed();
    }
}

/// The database job as one of the jobs the System page lists.
impl Job for MetadataJobs {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "MangaBaka database"
    }

    fn status(&self) -> JobStatus {
        self.inner.status().clone()
    }

    fn run(&self) -> Result<(), JobError> {
        self.download().map_err(|e| match e {
            MetadataJobError::AlreadyRunning => JobError::AlreadyRunning,
            e => JobError::Failed(e.to_string()),
        })
    }

    fn cancel(&self) -> Result<(), JobError> {
        MetadataJobs::cancel(self).map_err(|_| JobError::NotRunning)
    }
}
