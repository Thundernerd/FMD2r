//! Jobs a request starts rather than a schedule (an FMD2 import, a move of downloads), so the
//! System page shows them with their progress.

use std::sync::{Arc, Mutex, PoisonError};

use fmd_core::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};

/// A job registered on its first run. Cheap to clone.
#[derive(Clone)]
pub(crate) struct OneOffJob {
    id: &'static str,
    title: &'static str,
    /// Why `run` is refused: how a run starts instead.
    starts: &'static str,
    /// Why `cancel` is refused while it runs.
    uncancellable: &'static str,
    status: Arc<Mutex<JobStatus>>,
}

impl OneOffJob {
    pub(crate) fn new(
        id: &'static str,
        title: &'static str,
        starts: &'static str,
        uncancellable: &'static str,
    ) -> Self {
        Self {
            id,
            title,
            starts,
            uncancellable,
            status: Arc::new(Mutex::new(fresh_status(JobPhase::Idle, None))),
        }
    }

    pub(crate) fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        f(&mut self.status.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Starts a run unless one is running, registering the job in `jobs` on its first run;
    /// returns the status it replaced.
    pub(crate) fn begin(&self, jobs: &JobRegistry) -> Result<JobStatus, JobError> {
        let previous = {
            let mut current = self.status.lock().unwrap_or_else(PoisonError::into_inner);
            if current.phase == JobPhase::Running {
                return Err(JobError::AlreadyRunning);
            }
            std::mem::replace(
                &mut *current,
                fresh_status(JobPhase::Running, Some(now_ms())),
            )
        };
        if jobs.get(self.id).is_none() {
            jobs.register(self.clone());
        }
        jobs.changed(self.id);
        Ok(previous)
    }
}

/// A run's status before any progress.
fn fresh_status(phase: JobPhase, last_run: Option<i64>) -> JobStatus {
    JobStatus {
        phase,
        done: 0,
        total: 0,
        last_run,
        next_run: None,
        last_error: None,
    }
}

impl Job for OneOffJob {
    fn id(&self) -> &str {
        self.id
    }

    fn title(&self) -> &str {
        self.title
    }

    fn status(&self) -> JobStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn run(&self) -> Result<(), JobError> {
        Err(JobError::Unsupported(self.starts.into()))
    }

    fn cancel(&self) -> Result<(), JobError> {
        match self.status().phase {
            JobPhase::Running => Err(JobError::Unsupported(self.uncancellable.into())),
            _ => Err(JobError::NotRunning),
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
