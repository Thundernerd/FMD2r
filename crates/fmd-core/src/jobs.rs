//! Background jobs (favorites check, list update, module updater, ...) and the registry the
//! server lists and controls them through.
//!
//! A job registers itself once with [`JobRegistry::register`] and calls
//! [`JobRegistry::changed`] whenever its [`JobStatus`] moves, so the UI hears about it live.

use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};

use serde::Serialize;
use thiserror::Error;
use tokio::sync::broadcast;
use utoipa::ToSchema;

/// How many status changes a slow listener may fall behind before it misses some.
const CHANGES_CAPACITY: usize = 256;

/// What a background job is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JobPhase {
    /// Not running; waiting for its next scheduled or manual run.
    Idle,
    Running,
    /// The last run finished.
    Done,
    /// The last run stopped with an error ([`JobStatus::last_error`]).
    Failed,
}

/// A snapshot of a job, as the System page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct JobStatus {
    pub phase: JobPhase,
    /// Work items finished in the current (or last) run.
    pub done: u64,
    /// Work items in the current (or last) run; 0 when unknown.
    pub total: u64,
    /// When the last run started, in Unix milliseconds.
    pub last_run: Option<i64>,
    /// When the next scheduled run starts, in Unix milliseconds.
    pub next_run: Option<i64>,
    /// Why the last run failed.
    pub last_error: Option<String>,
}

/// Why a job could not be started or cancelled.
#[derive(Debug, Error)]
pub enum JobError {
    #[error("job is already running")]
    AlreadyRunning,
    #[error("job is not running")]
    NotRunning,
    /// The job does not support the request, e.g. it cannot be started on its own.
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Failed(String),
}

/// A background job the server can list, start and cancel.
pub trait Job: Send + Sync + 'static {
    /// Stable identifier, used in `/api/jobs/{id}/...`.
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    /// Must be cheap: it is called on the async threads, for every listing and change.
    fn status(&self) -> JobStatus;
    /// Starts a run now and returns without waiting for it to finish.
    fn run(&self) -> Result<(), JobError>;
    /// Asks the running run to stop; returns without waiting for it.
    fn cancel(&self) -> Result<(), JobError>;
}

/// The jobs known to the server, in registration order. Cheap to clone.
#[derive(Clone)]
pub struct JobRegistry {
    jobs: Arc<RwLock<Vec<Arc<dyn Job>>>>,
    changes: broadcast::Sender<String>,
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: Arc::default(),
            changes: broadcast::channel(CHANGES_CAPACITY).0,
        }
    }

    /// Adds `job`, replacing a registered job with the same id.
    pub fn register(&self, job: impl Job) {
        let job: Arc<dyn Job> = Arc::new(job);
        // The list stays consistent even if a holder panicked, so a poisoned lock is still usable.
        let mut jobs = self.jobs.write().unwrap_or_else(PoisonError::into_inner);
        match jobs.iter_mut().find(|j| j.id() == job.id()) {
            Some(slot) => *slot = job,
            None => jobs.push(job),
        }
    }

    /// Every registered job, in registration order.
    pub fn list(&self) -> Vec<Arc<dyn Job>> {
        self.read().clone()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Job>> {
        self.read().iter().find(|j| j.id() == id).cloned()
    }

    fn read(&self) -> RwLockReadGuard<'_, Vec<Arc<dyn Job>>> {
        self.jobs.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Announces that the status of job `id` changed.
    pub fn changed(&self, id: &str) {
        let _ = self.changes.send(id.to_owned());
    }

    /// Ids of jobs whose status changed, as [`JobRegistry::changed`] announces them.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.changes.subscribe()
    }
}
