//! List updates and FMD2-DB imports as background jobs, one at a time per module, reporting
//! their progress as [`ListEvent`]s.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use fmd_http::TerminateToken;
use fmd_lua::Module;
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use super::{DbImporter, ListPhase, ListProgress, ListUpdater, UpdateOptions};
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

impl ListEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Progress => "progress",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
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
    /// Why it failed.
    pub error: Option<String>,
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

type EventSink = dyn Fn(ListEvent) + Send + Sync;

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
    on_event: Box<EventSink>,
}

impl ListJobs {
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
                on_event: Box::new(on_event),
            }),
        }
    }

    /// Starts updating `module_id`'s list on a thread of its own.
    pub fn update(&self, module_id: &str) -> Result<(), ListJobError> {
        let module = self
            .inner
            .modules
            .module(module_id)
            .ok_or_else(|| ListJobError::UnknownModule(module_id.into()))?;
        self.start(
            module_id,
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
                    .update(&module, &options, terminate, &mut progress)
                    .map_err(|e| e.to_string())?;
                Ok((outcome.added, outcome.cancelled))
            },
        )
    }

    /// Starts downloading and importing `module_id`'s FMD2-DB dump on a thread of its own.
    pub fn import_db(&self, module_id: &str) -> Result<(), ListJobError> {
        if self.inner.modules.module(module_id).is_none() {
            return Err(ListJobError::UnknownModule(module_id.into()));
        }
        let id = module_id.to_owned();
        self.start(
            module_id,
            ListJobKind::ImportDb,
            move |inner, terminate, _| {
                let url = inner.settings.get().update_lists.db_url.clone();
                match inner.importer.import(&id, &url, terminate) {
                    Ok(titles) => Ok((titles, false)),
                    Err(super::ImportError::Cancelled) => Ok((0, true)),
                    Err(e) => Err(e.to_string()),
                }
            },
        )
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

    /// Runs `work` on a new thread unless `module_id` already has a job, reporting it as
    /// `kind`. `work` returns the titles it added and whether it was cancelled.
    fn start(
        &self,
        module_id: &str,
        kind: ListJobKind,
        work: impl FnOnce(&Inner, &TerminateToken, &Events<'_>) -> Result<(u64, bool), String>
        + Send
        + 'static,
    ) -> Result<(), ListJobError> {
        let terminate = TerminateToken::new();
        {
            let mut running = self.inner.running();
            if running.contains_key(module_id) {
                return Err(ListJobError::AlreadyRunning(module_id.into()));
            }
            running.insert(module_id.to_owned(), terminate.clone());
        }
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
                inner.running().remove(&id);
                events.end(result);
            });
        if let Err(e) = spawned {
            self.inner.running().remove(module_id);
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

    fn end(&self, result: Result<(u64, bool), String>) {
        let event = match result {
            Ok((titles, cancelled)) => {
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
                    error: Some(error),
                    ..self.event(ListEventKind::Failed, String::new())
                }
            }
        };
        (self.inner.on_event)(event);
    }
}
