//! `THTTPQueue`: the per-module connection limit.

use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::terminate::TerminateToken;

/// At most `max_connections` requests of one module in flight; 0 means unlimited
/// (baseunits/httpsendthread.pas:404-443). A whole request, retries and redirects
/// included, holds one slot (baseunits/httpsendthread.pas:578-590).
#[derive(Default)]
pub(crate) struct ConnectionQueue {
    state: Mutex<QueueState>,
    released: Notify,
}

#[derive(Default)]
struct QueueState {
    active: u32,
    max: u32,
}

impl ConnectionQueue {
    pub(crate) fn max_connections(&self) -> u32 {
        self.state.lock().map_or(0, |s| s.max)
    }

    pub(crate) fn set_max_connections(&self, max: u32) {
        if let Ok(mut s) = self.state.lock() {
            s.max = max;
        }
        // A raised limit may admit waiters.
        self.released.notify_waiters();
    }

    /// Waits for a free slot. Unlike FMD2's polling loop, the wait ends early (with
    /// `None`) when `terminate` fires.
    pub(crate) async fn acquire(self: &Arc<Self>, terminate: &TerminateToken) -> Option<Slot> {
        loop {
            let released = self.released.notified();
            tokio::pin!(released);
            released.as_mut().enable();
            if terminate.is_terminated() {
                return None;
            }
            if self.try_take() {
                return Some(Slot(self.clone()));
            }
            tokio::select! {
                _ = released => {}
                _ = terminate.terminated() => return None,
            }
        }
    }

    fn try_take(&self) -> bool {
        // A poisoned lock only means another thread panicked mid-update; the counters
        // are still meaningful.
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.max == 0 || s.active < s.max {
            s.active += 1;
            true
        } else {
            false
        }
    }
}

/// A held connection slot, released on drop (`DoneConnection`).
pub(crate) struct Slot(Arc<ConnectionQueue>);

impl Drop for Slot {
    fn drop(&mut self) {
        let mut s = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        s.active = s.active.saturating_sub(1);
        drop(s);
        self.0.released.notify_waiters();
    }
}
