//! List updates and FMD2-DB imports as background jobs, one at a time per module, reporting
//! their progress as [`ListEvent`]s. Together they are the `lists` job of the [`JobRegistry`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use fmd_http::TerminateToken;
use fmd_lua::{Module, ModuleDef};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use super::{
    DbImporter, ImportError, ListError, ListPhase, ListProgress, ListUpdater, UpdateOptions,
};
use crate::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use crate::settings::SettingsService;

/// Which job a [`ListEvent`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListJobKind {
    /// Running the module's update-list callbacks ([`ListUpdater`]).
    Update,
    /// Downloading and importing its FMD2-DB dump ([`DbImporter`]).
    ImportDb,
}

/// What happened to a list job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListEventKind {
    Started,
    Progress,
    Finished,
    Cancelled,
    Failed,
}

/// One step of a list job, for the Discover page's progress (`job.lists.<kind>` events).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ListEvent {
    pub module_id: String,
    pub job: ListJobKind,
    pub kind: ListEventKind,
    /// FMD2's status text, or the module's own.
    pub status_text: String,
    /// Work items of the current step done.
    pub done: u64,
    /// Work items of the current step; 0 when unknown.
    pub total: u64,
    /// Titles added (update) or imported (import), once finished.
    pub titles: Option<u64>,
    /// Why it failed: the technical details.
    pub error: Option<String>,
    /// What kind of failure it was, to pick a message for `error` by.
    pub reason: Option<ListFailureReason>,
}

/// Why a list job failed, for the UI to pick its message from rather than parse
/// [`ListEvent::error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListFailureReason {
    /// FMD2-DB has no dump for the module (its URL answers 404).
    NoDump,
    /// The website, or FMD2-DB's host, could not be reached: a connection error, or an error
    /// status other than 404.
    Unreachable,
    /// The download was not a 7z archive holding a usable database, or was empty.
    BadArchive,
    /// Anything else: writing the list, or the module's update-list callbacks.
    Failed,
}

/// Why a list job could not be started or cancelled.
#[derive(Debug, Error)]
pub enum ListJobError {
    #[error("no module {0} is loaded")]
    UnknownModule(String),
    #[error("a list job of module {0} is already running")]
    AlreadyRunning(String),
    #[error("no list job of module {0} is running")]
    NotRunning(String),
    #[error("starting the job thread: {0}")]
    Spawn(#[from] std::io::Error),
}

/// The loaded modules, looked up when a job starts.
pub trait ListModules: Send + Sync + 'static {
    fn module(&self, id: &str) -> Option<Arc<Module>>;
}

/// A lookup like the download engine's [`crate::download::ModuleLookup`].
impl<F> ListModules for F
where
    F: Fn(&str) -> Option<Arc<Module>> + Send + Sync + 'static,
{
    fn module(&self, id: &str) -> Option<Arc<Module>> {
        self(id)
    }
}

type EventSink = dyn Fn(ListEvent) + Send + Sync;

/// How a list job that did not fail ended.
struct JobOutcome {
    /// Titles added (update) or imported (import).
    titles: u64,
    cancelled: bool,
}

/// Why a list job failed.
#[derive(Debug, Error)]
enum JobFailure {
    #[error(transparent)]
    Update(#[from] ListError),
    #[error(transparent)]
    Import(#[from] ImportError),
}

impl JobFailure {
    fn reason(&self) -> ListFailureReason {
        match self {
            Self::Update(_) => ListFailureReason::Failed,
            Self::Import(e) => e.reason(),
        }
    }
}

impl ListFailureReason {
    /// What the user reads about a `job` of `module` that failed this way. The web UI
    /// words it the same (`web/src/lib/components/discover/ListActions.svelte`).
    fn message(self, job: ListJobKind, module: &ModuleDef) -> String {
        let website = &module.name;
        match (self, job) {
            (Self::NoDump, _) if module.on_get_name_and_link.is_some() => format!(
                "FMD2-DB has no ready-made list for {website}. \
                 Use Update list to build it from the website."
            ),
            (Self::NoDump, _) => format!(
                "FMD2-DB has no ready-made list for {website}, \
                 and this website cannot build one itself."
            ),
            (Self::Unreachable, _) => format!(
                "Could not reach FMD2-DB to get the list of {website}. \
                 Check the connection and try again later."
            ),
            (Self::BadArchive, _) => {
                format!("The list FMD2-DB sent for {website} is damaged or empty.")
            }
            (Self::Failed, ListJobKind::Update) => {
                format!("Updating the list of {website} failed.")
            }
            (Self::Failed, ListJobKind::ImportDb) => {
                format!("Getting the list of {website} from FMD2-DB failed.")
            }
        }
    }
}

/// Starts and cancels list jobs. Cheap to clone.
#[derive(Clone)]
pub struct ListJobs {
    inner: Arc<Inner>,
}

struct Inner {
    updater: ListUpdater,
    importer: DbImporter,
    settings: Arc<SettingsService>,
    modules: Arc<dyn ListModules>,
    running: Mutex<HashMap<String, TerminateToken>>,
    /// The `lists` job: the list jobs since none was running. Locked after `running`.
    status: Mutex<JobStatus>,
    /// Where the `lists` job announces its changes, once registered.
    registry: OnceLock<JobRegistry>,
    on_event: Box<EventSink>,
}

impl ListJobs {
    /// The `lists` job's id in `/api/jobs/{id}`.
    pub const ID: &str = "lists";

    /// Jobs reading their options from `settings` (`connections.max_update_list_threads`,
    /// `update_lists.no_manga_info`, `update_lists.db_url`) and passing every event to
    /// `on_event`.
    pub fn new(
        updater: ListUpdater,
        importer: DbImporter,
        settings: Arc<SettingsService>,
        modules: impl ListModules,
        on_event: impl Fn(ListEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                updater,
                importer,
                settings,
                modules: Arc::new(modules),
                running: Mutex::default(),
                status: Mutex::new(JobStatus {
                    phase: JobPhase::Idle,
                    done: 0,
                    total: 0,
                    last_run: None,
                    next_run: None,
                    last_error: None,
                }),
                registry: OnceLock::new(),
                on_event: Box::new(on_event),
            }),
        }
    }

    /// Starts updating `module_id`'s list on a thread of its own.
    pub fn update(&self, module_id: &str) -> Result<(), ListJobError> {
        let module = self.module(module_id)?;
        let def = module.def();
        self.start(
            module_id,
            def,
            ListJobKind::Update,
            move |inner, terminate, events| {
                let settings = inner.settings.get();
                let options = UpdateOptions {
                    max_threads: settings.connections.max_update_list_threads,
                    no_manga_info: settings.update_lists.no_manga_info,
                };
                let mut progress = |p: &ListProgress| events.progress(p);
                let outcome = inner
                    .updater
                    .update(&module, &options, terminate, &mut progress)?;
                Ok(JobOutcome {
                    titles: outcome.added,
                    cancelled: outcome.cancelled,
                })
            },
        )
    }

    /// Starts downloading and importing `module_id`'s FMD2-DB dump on a thread of its own.
    pub fn import_db(&self, module_id: &str) -> Result<(), ListJobError> {
        let def = self.module(module_id)?.def();
        let id = module_id.to_owned();
        self.start(
            module_id,
            def,
            ListJobKind::ImportDb,
            move |inner, terminate, events| {
                let url = inner.settings.get().update_lists.db_url.clone();
                let mut status = |text: &str| events.send(ListEventKind::Progress, text.into());
                match inner.importer.import(&id, &url, terminate, &mut status) {
                    Ok(titles) => Ok(JobOutcome {
                        titles,
                        cancelled: false,
                    }),
                    Err(ImportError::Cancelled) => Ok(JobOutcome {
                        titles: 0,
                        cancelled: true,
                    }),
                    Err(e) => Err(e.into()),
                }
            },
        )
    }

    /// The loaded module `module_id`.
    fn module(&self, module_id: &str) -> Result<Arc<Module>, ListJobError> {
        self.inner
            .modules
            .module(module_id)
            .ok_or_else(|| ListJobError::UnknownModule(module_id.into()))
    }

    /// Asks the list job of `module_id` to stop.
    pub fn cancel(&self, module_id: &str) -> Result<(), ListJobError> {
        let running = self.inner.running();
        let token = running
            .get(module_id)
            .ok_or_else(|| ListJobError::NotRunning(module_id.into()))?;
        token.terminate();
        Ok(())
    }

    /// Whether a list job of `module_id` is running.
    pub fn is_running(&self, module_id: &str) -> bool {
        self.inner.running().contains_key(module_id)
    }

    /// Runs `work` on a new thread unless `module_id` (defined by `def`) already has a job,
    /// reporting it as `kind`.
    fn start(
        &self,
        module_id: &str,
        def: ModuleDef,
        kind: ListJobKind,
        work: impl FnOnce(&Inner, &TerminateToken, &Events<'_>) -> Result<JobOutcome, JobFailure>
        + Send
        + 'static,
    ) -> Result<(), ListJobError> {
        let terminate = TerminateToken::new();
        {
            let mut running = self.inner.running();
            if running.contains_key(module_id) {
                return Err(ListJobError::AlreadyRunning(module_id.into()));
            }
            let mut status = self.inner.status();
            if running.is_empty() {
                status.done = 0;
                status.total = 0;
                status.last_error = None;
            }
            status.phase = JobPhase::Running;
            status.total += 1;
            status.last_run = Some(now_ms());
            running.insert(module_id.to_owned(), terminate.clone());
        }
        self.inner.changed();
        let inner = self.inner.clone();
        let id = module_id.to_owned();
        let spawned = std::thread::Builder::new()
            .name(format!("fmd-list-{id}"))
            .spawn(move || {
                let events = Events {
                    inner: &inner,
                    module_id: &id,
                    kind,
                };
                events.send(ListEventKind::Started, "Preparing...".into());
                let result = work(&inner, &terminate, &events);
                // Out of `running` before the last event, so a client may start the next job
                // as soon as it hears this one ended.
                let error = result.as_ref().err().map(|e| {
                    let message = e.reason().message(kind, &def);
                    format!("{message}\n\nDetails: {id}: {e}")
                });
                inner.finished(&id, error);
                events.end(result);
            });
        if let Err(e) = spawned {
            self.inner
                .finished(module_id, Some(format!("{module_id}: {e}")));
            return Err(e.into());
        }
        Ok(())
    }
}

impl Inner {
    fn running(&self) -> std::sync::MutexGuard<'_, HashMap<String, TerminateToken>> {
        // The map stays consistent even if a holder panicked.
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn status(&self) -> std::sync::MutexGuard<'_, JobStatus> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends `module_id`'s job, which failed with `error` when one is given.
    fn finished(&self, module_id: &str, error: Option<String>) {
        {
            let mut running = self.running();
            running.remove(module_id);
            let mut status = self.status();
            status.done += 1;
            if error.is_some() {
                status.last_error = error;
            }
            if running.is_empty() {
                status.phase = if status.last_error.is_some() {
                    JobPhase::Failed
                } else {
                    JobPhase::Done
                };
            }
        }
        self.changed();
    }

    fn changed(&self) {
        if let Some(registry) = self.registry.get() {
            registry.changed(ListJobs::ID);
        }
    }
}

impl ListJobs {
    /// Adds the list jobs to `registry` as the `lists` job and announces their changes there.
    pub fn register(&self, registry: &JobRegistry) {
        // Registered once; a second registry is not told about changes.
        let _ = self.inner.registry.set(registry.clone());
        registry.register(self.clone());
        registry.changed(Self::ID);
    }
}

/// The list jobs as one job, like FMD2's single update-list thread working through the chosen
/// websites (`TUpdateListManagerThread.Execute`, baseunits/uUpdateThread.pas:626-779): running while any module's
/// list job runs, counting the jobs since none was running.
impl Job for ListJobs {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "Update lists"
    }

    fn status(&self) -> JobStatus {
        self.inner.status().clone()
    }

    /// A list job is for one module: it starts from `POST /api/lists/{module}/...`.
    fn run(&self) -> Result<(), JobError> {
        Err(JobError::Unsupported(
            "list updates start per module, from the Discover page".into(),
        ))
    }

    /// Asks every running list job to stop.
    fn cancel(&self) -> Result<(), JobError> {
        let running = self.inner.running();
        if running.is_empty() {
            return Err(JobError::NotRunning);
        }
        for token in running.values() {
            token.terminate();
        }
        Ok(())
    }
}

/// Now, in Unix milliseconds.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Sends the events of one job.
struct Events<'a> {
    inner: &'a Inner,
    module_id: &'a str,
    kind: ListJobKind,
}

impl Events<'_> {
    fn event(&self, kind: ListEventKind, status_text: String) -> ListEvent {
        ListEvent {
            module_id: self.module_id.to_owned(),
            job: self.kind,
            kind,
            status_text,
            done: 0,
            total: 0,
            titles: None,
            error: None,
            reason: None,
        }
    }

    fn send(&self, kind: ListEventKind, status_text: String) {
        (self.inner.on_event)(self.event(kind, status_text));
    }

    fn progress(&self, progress: &ListProgress) {
        let mut event = self.event(ListEventKind::Progress, progress.status_text.clone());
        if progress.phase != ListPhase::Preparing {
            event.done = progress.done;
            event.total = progress.total;
        }
        (self.inner.on_event)(event);
    }

    fn end(&self, result: Result<JobOutcome, JobFailure>) {
        let event = match result {
            Ok(JobOutcome { titles, cancelled }) => {
                let kind = if cancelled {
                    ListEventKind::Cancelled
                } else {
                    ListEventKind::Finished
                };
                ListEvent {
                    titles: Some(titles),
                    ..self.event(kind, String::new())
                }
            }
            Err(error) => {
                tracing::warn!(target: "fmd_core", "list job of {}: {error}", self.module_id);
                ListEvent {
                    error: Some(error.to_string()),
                    reason: Some(error.reason()),
                    ..self.event(ListEventKind::Failed, String::new())
                }
            }
        };
        (self.inner.on_event)(event);
    }
}
