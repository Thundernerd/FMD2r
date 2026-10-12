//! The MangaBaka database as a background job: download, refresh, and matching lists and
//! favorites.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use fmd_http::TerminateToken;
use fmd_store::ListsDb;
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use super::db::now_ms;
use super::matcher::absolute;
use super::{
    BuildProgress, DbInfo, ListModule, MangaBakaDb, MatchScope, Matcher, Metadata, MetadataError,
};
use crate::download::SeriesMatch;
use crate::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use crate::settings::SettingsService;

const SCHEDULE_TICK: Duration = Duration::from_secs(60 * 60);
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// `link` as given, then made absolute against `root`, then relative to it.
fn link_forms(link: &str, root: Option<&str>) -> Vec<String> {
    let mut forms = vec![link.to_owned()];
    if let Some(root) = root
        .map(|r| r.trim_end_matches('/'))
        .filter(|r| !r.is_empty())
    {
        forms.push(absolute(root, link));
        if let Some(path) = link.strip_prefix(root).filter(|p| p.starts_with('/')) {
            forms.push(path.to_owned());
        }
    }
    forms.dedup();
    forms
}

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
    /// Matching the lists and favorites against the new database; `done` and `total` count
    /// modules.
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

#[derive(Debug, Error)]
pub enum MetadataJobError {
    #[error("the MangaBaka database is already being downloaded")]
    AlreadyRunning,
    #[error("the MangaBaka database is not being downloaded")]
    NotRunning,
    #[error("the MangaBaka database is not downloaded")]
    NoDatabase,
    #[error("MangaBaka series {0} is not in the database")]
    UnknownSeries(i64),
    #[error("starting the job thread: {0}")]
    Spawn(#[from] std::io::Error),
    #[error(transparent)]
    Metadata(#[from] MetadataError),
}

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
    running: Mutex<Option<TerminateToken>>,
    status: Mutex<JobStatus>,
    /// The schedule waits a day after a failure.
    failed_at: Mutex<Option<i64>>,
    /// For a page opened while the download runs.
    last_event: Mutex<Option<MetadataEvent>>,
    /// So a list update and a refresh do not match the same list at once.
    matching: Mutex<()>,
    registry: OnceLock<JobRegistry>,
    on_event: Box<EventSink>,
}

impl MetadataJobs {
    pub const ID: &str = "mangabaka";

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

    pub fn current(&self) -> Option<Arc<Metadata>> {
        self.inner.db.current()
    }

    pub fn info(&self) -> Option<DbInfo> {
        self.inner.db.info()
    }

    /// The accepted match of `module_id`'s series `link`, with its cross-site IDs. `None` when
    /// there is none, or reading lists.db or metadata.db failed (logged). A download's link is
    /// the one the module reported, which may be absolute where the list's is relative, or the
    /// other way round; both forms are tried.
    pub fn series_match(&self, module_id: &str, link: &str) -> Option<SeriesMatch> {
        let found = (|| -> Result<Option<SeriesMatch>, fmd_store::StoreError> {
            let Some(db) = self.inner.db.current() else {
                return Ok(None);
            };
            let root = self.inner.modules.root_url(module_id);
            for link in link_forms(link, root.as_deref()) {
                let Some(stored) = self.inner.lists.matches().get(module_id, &link)? else {
                    continue;
                };
                let Some(id) = stored.series_id.filter(|_| stored.confidence.is_accepted()) else {
                    return Ok(None);
                };
                let Some(series) = db.series(id)? else {
                    return Ok(None);
                };
                let xids = db.xids_of(id)?;
                return Ok(Some(SeriesMatch { series, xids }));
            }
            Ok(None)
        })();
        found.unwrap_or_else(|e| {
            tracing::warn!(target: "fmd_core", "MangaBaka match of {module_id} {link}: {e}");
            None
        })
    }

    /// Matches `module_id`'s series `link` to MangaBaka series `series_id` by hand. The match
    /// outlives matching, list updates, refreshes and removing the database. Blocking.
    pub fn set_override(
        &self,
        module_id: &str,
        link: &str,
        series_id: i64,
    ) -> Result<(), MetadataJobError> {
        let _matching = lock(&self.inner.matching);
        let meta = self
            .inner
            .db
            .current()
            .ok_or(MetadataJobError::NoDatabase)?;
        if !self
            .inner
            .matcher
            .set_override(&meta, module_id, link, series_id)?
        {
            return Err(MetadataJobError::UnknownSeries(series_id));
        }
        Ok(())
    }

    /// Forgets the MangaBaka series picked by hand for `module_id`'s series `link`, and matches
    /// the module's titles again. Blocking.
    pub fn remove_override(&self, module_id: &str, link: &str) -> Result<(), MetadataJobError> {
        let removed = {
            let _matching = lock(&self.inner.matching);
            self.inner.matcher.remove_override(module_id, link)?
        };
        if removed {
            self.list_changed(module_id, &TerminateToken::new());
        }
        Ok(())
    }

    /// The MangaBaka series picked by hand for `module_id`'s series `link`, whether or not the
    /// database has it.
    pub fn override_of(
        &self,
        module_id: &str,
        link: &str,
    ) -> Result<Option<i64>, MetadataJobError> {
        Ok(self.inner.matcher.override_of(module_id, link)?)
    }

    pub fn is_running(&self) -> bool {
        lock(&self.inner.running).is_some()
    }

    pub fn progress(&self) -> Option<MetadataEvent> {
        if self.is_running() {
            lock(&self.inner.last_event).clone()
        } else {
            None
        }
    }

    /// Downloads the database on a new thread, then matches every list against it.
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

    /// The database it would have replaced stays.
    pub fn cancel(&self) -> Result<(), MetadataJobError> {
        match lock(&self.inner.running).as_ref() {
            Some(token) => {
                token.terminate();
                Ok(())
            }
            None => Err(MetadataJobError::NotRunning),
        }
    }

    /// Also deletes every match.
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

    /// Matches new or changed titles of the module's list and favorites. Blocking.
    pub fn list_changed(&self, module_id: &str, terminate: &TerminateToken) {
        // Locked first, so a refresh that holds it is not undone with its old database.
        let _matching = lock(&self.inner.matching);
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

    /// [`MetadataJobs::list_changed`] after a favorite of the module was added or its website
    /// info changed. Blocking.
    pub fn favorites_changed(&self, module_id: &str) {
        self.list_changed(module_id, &TerminateToken::new());
    }

    /// Matches list titles and favorites still owing a match: new, changed, matched against an older database, or
    /// matched while MangaDex could not be asked. Blocking.
    pub fn catch_up(&self) {
        let module_ids = match self.inner.matcher.module_ids() {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!(target: "fmd_core", "catching up MangaBaka matches: {e}");
                return;
            }
        };
        let terminate = TerminateToken::new();
        for module_id in module_ids {
            if self.is_running() {
                return;
            }
            self.list_changed(&module_id, &terminate);
        }
    }

    pub fn register(&self, registry: &JobRegistry) {
        // Registered once; a second registry is not told about changes.
        let _ = self.inner.registry.set(registry.clone());
        registry.register(self.clone());
        registry.changed(Self::ID);
    }

    /// Refreshes a downloaded database every `refresh_days` (0 = never; a day after a failure),
    /// rebuilds one with an older schema at startup, and runs [`MetadataJobs::catch_up`] at
    /// startup and hourly. Runs until dropped.
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

    /// A database with an older schema is due at once, whatever `refresh_days` says.
    fn next_refresh(&self) -> Option<i64> {
        let retry = lock(&self.failed_at).map(|at| at.saturating_add(DAY_MS));
        if self.db.is_outdated() {
            return Some(retry.unwrap_or_else(now_ms));
        }
        let days = self.settings.get().metadata.mangabaka.refresh_days;
        if days == 0 {
            return None;
        }
        let built_at = self.db.info()?.built_at;
        let due = built_at.saturating_add(i64::from(days) * DAY_MS);
        Some(due.max(retry.unwrap_or(due)))
    }

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
            Err(MetadataError::Cancelled) => {
                self.db.forget_outdated();
                return self.end_cancelled(MetadataPhase::Downloading);
            }
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

    /// Returns whether it was cancelled.
    fn match_all(&self, terminate: &TerminateToken) -> Result<bool, MetadataError> {
        let Some(meta) = self.db.current() else {
            return Ok(false);
        };
        let modules: Vec<ListModule> = self
            .matcher
            .module_ids()?
            .into_iter()
            .filter_map(|id| {
                let root_url = self.modules.root_url(&id)?;
                Some(ListModule { id, root_url })
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
