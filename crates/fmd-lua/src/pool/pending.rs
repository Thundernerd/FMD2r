//! The result of a queued job, to await or to wait for.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use fmd_http::TerminateToken;
use tokio::sync::oneshot;

use super::{JobError, JobResult};

type Map<T> = Box<dyn FnOnce(JobResult) -> Result<T, JobError> + Send>;

/// A job queued on a [`WorkerPool`](super::WorkerPool): await it, or [`wait`](Self::wait) for
/// it on a thread outside any tokio runtime. Dropping it does not cancel the job;
/// [`terminate`](Self::terminate) does.
#[must_use = "a job's result is only seen through its Pending"]
pub struct Pending<T> {
    receiver: oneshot::Receiver<Result<JobResult, JobError>>,
    terminate: TerminateToken,
    map: Option<Map<T>>,
}

impl<T> Pending<T> {
    pub(super) fn new(
        receiver: oneshot::Receiver<Result<JobResult, JobError>>,
        terminate: TerminateToken,
        map: impl FnOnce(JobResult) -> Result<T, JobError> + Send + 'static,
    ) -> Pending<T> {
        Pending {
            receiver,
            terminate,
            map: Some(Box::new(map)),
        }
    }

    pub(super) fn map<U>(
        mut self,
        f: impl FnOnce(T) -> Result<U, JobError> + Send + 'static,
    ) -> Pending<U>
    where
        T: 'static,
    {
        let map = self.map.take();
        Pending {
            receiver: self.receiver,
            terminate: self.terminate,
            map: Some(Box::new(move |result| match map {
                Some(map) => f(map(result)?),
                None => Err(JobError::Closed),
            })),
        }
    }

    /// Terminates the job: its HTTP requests, `sleep` and `ExecJS` are cut short.
    pub fn terminate(&self) {
        self.terminate.terminate();
    }

    /// Blocks until the job is done. Fails with [`JobError::InsideRuntime`] on a thread inside
    /// a tokio runtime, which must `.await` instead.
    pub fn wait(self) -> Result<T, JobError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(JobError::InsideRuntime);
        }
        let result = self.receiver.blocking_recv();
        finish(self.map, result.map_err(|_| JobError::Closed))
    }
}

fn finish<T>(
    map: Option<Map<T>>,
    result: Result<Result<JobResult, JobError>, JobError>,
) -> Result<T, JobError> {
    let result = result??;
    match map {
        Some(map) => map(result),
        None => Err(JobError::Closed),
    }
}

impl<T> Future for Pending<T> {
    type Output = Result<T, JobError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.receiver).poll(cx) {
            Poll::Ready(result) => Poll::Ready(finish(
                self.map.take(),
                result.map_err(|_| JobError::Closed),
            )),
            Poll::Pending => Poll::Pending,
        }
    }
}
