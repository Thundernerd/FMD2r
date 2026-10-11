//! The websites that rate-limited this server, and until when background requests to them pause.
//! Beyond FMD2, which keeps asking.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::InfoError;

/// How long a website stays paused after a rate-limited answer without a `Retry-After`.
const DEFAULT_PAUSE: Duration = Duration::from_secs(10 * 60);

/// Per module ID: until when its cover lookups and favorite checks pause, and the
/// [`InfoError::RateLimited`] that started the pause.
#[derive(Debug, Default)]
pub struct RateLimits {
    paused: Mutex<HashMap<String, (Instant, InfoError)>>,
}

impl RateLimits {
    /// When `err` is [`InfoError::RateLimited`], pauses `module_id` until its `Retry-After`, else
    /// for [`DEFAULT_PAUSE`]; a pause already running is only extended. Other errors do nothing.
    /// Logs a warning when a pause starts.
    pub fn pause_if_limited(&self, module_id: &str, err: &InfoError) {
        let InfoError::RateLimited {
            website,
            status,
            retry_after,
        } = err
        else {
            return;
        };
        let pause = retry_after.unwrap_or(DEFAULT_PAUSE);
        let until = Instant::now() + pause;
        let mut paused = lock(&self.paused);
        match paused.get_mut(module_id) {
            Some((current, _)) if *current > Instant::now() => *current = (*current).max(until),
            _ => {
                tracing::warn!(
                    target: "fmd_core",
                    "{website} answered {status}: pausing its cover lookups and favorite checks for {}s",
                    pause.as_secs()
                );
                paused.insert(module_id.to_owned(), (until, err.clone()));
            }
        }
    }

    /// The rate-limited answer that paused `module_id`, while the pause lasts.
    pub fn paused(&self, module_id: &str) -> Option<InfoError> {
        let paused = lock(&self.paused);
        let (until, err) = paused.get(module_id)?;
        (*until > Instant::now()).then(|| err.clone())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
