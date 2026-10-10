//! Background jobs and the registry the server lists and controls them through.

use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};

use serde::Serialize;
use thiserror::Error;
use tokio::sync::broadcast;
use utoipa::ToSchema;

/// How far a slow listener may fall behind before it misses changes.
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

#[derive(Debug, Clone, PartialEq)]
pub struct JobStatus {
    pub phase: JobPhase,
    /// Work items finished in the current (or last) run.
    pub done: u64,
    /// Work items in the current (or last) run; 0 when unknown.
    pub total: u64,
    /// Unix milliseconds.
    pub last_run: Option<i64>,
    /// Unix milliseconds.
    pub next_run: Option<i64>,
    pub last_error: Option<String>,
}

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

pub trait Job: Send + Sync + 'static {
    /// Used in `/api/jobs/{id}/...`.
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    /// Must be cheap: called on the async threads.
    fn status(&self) -> JobStatus;
    /// Returns without waiting for the run.
    fn run(&self) -> Result<(), JobError>;
    /// Returns without waiting for the run to stop.
    fn cancel(&self) -> Result<(), JobError>;
}

/// In registration order. Cheap to clone.
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

    /// Replaces a registered job with the same id.
    pub fn register(&self, job: impl Job) {
        let job: Arc<dyn Job> = Arc::new(job);
        // The list stays consistent even if a holder panicked.
        let mut jobs = self.jobs.write().unwrap_or_else(PoisonError::into_inner);
        match jobs.iter_mut().find(|j| j.id() == job.id()) {
            Some(slot) => *slot = job,
            None => jobs.push(job),
        }
    }

    pub fn list(&self) -> Vec<Arc<dyn Job>> {
        self.read().clone()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Job>> {
        self.read().iter().find(|j| j.id() == id).cloned()
    }

    fn read(&self) -> RwLockReadGuard<'_, Vec<Arc<dyn Job>>> {
        self.jobs.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Call whenever job `id`'s status moves, so the UI hears about it live.
    pub fn changed(&self, id: &str) {
        let _ = self.changes.send(id.to_owned());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.changes.subscribe()
    }
}
