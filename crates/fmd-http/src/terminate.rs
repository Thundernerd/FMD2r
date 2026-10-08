//! Cancellation shared between a session and whoever owns its worker.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// Terminates a session from another thread: aborts queue waits, retries, redirects
/// and in-flight requests, like FMD2's owner-thread termination plus `Stop`
/// (baseunits/httpsendthread.pas:453-458, 810-825). Termination is permanent.
#[derive(Clone, Default)]
pub struct TerminateToken {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    terminated: AtomicBool,
    notify: Notify,
}

impl TerminateToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn terminate(&self) {
        self.inner.terminated.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_terminated(&self) -> bool {
        self.inner.terminated.load(Ordering::SeqCst)
    }

    /// Completes once [`terminate`](Self::terminate) has been called.
    pub(crate) async fn terminated(&self) {
        loop {
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_terminated() {
                return;
            }
            notified.await;
        }
    }
}
