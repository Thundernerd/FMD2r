//! FMD2's `TCriticalSection`, the `Guardian` objects modules lock explicitly with `Enter` and
//! `Leave` (baseunits/lua/LuaCriticalSection.pas:21-55).

use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;

/// A recursive lock entered and left by explicit calls rather than a guard, like FPC's
/// `TCriticalSection`: the thread holding it may enter again, and must leave as often as it
/// entered before another thread gets in.
#[derive(Default)]
pub struct CriticalSection {
    owner: Mutex<Owner>,
    released: Condvar,
}

#[derive(Default)]
struct Owner {
    thread: Option<ThreadId>,
    depth: u32,
}

impl CriticalSection {
    /// Blocks until the calling thread holds the lock (`Enter`).
    pub fn enter(&self) {
        let me = std::thread::current().id();
        let mut owner = super::lock(&self.owner);
        while owner.thread.is_some_and(|t| t != me) {
            owner = self.released.wait(owner).unwrap_or_else(|e| e.into_inner());
        }
        owner.thread = Some(me);
        owner.depth += 1;
    }

    /// Takes the lock when no other thread holds it, returning whether it did (`TryEnter`).
    pub fn try_enter(&self) -> bool {
        let me = std::thread::current().id();
        let mut owner = super::lock(&self.owner);
        if owner.thread.is_some_and(|t| t != me) {
            return false;
        }
        owner.thread = Some(me);
        owner.depth += 1;
        true
    }

    /// Releases one `enter` of the calling thread (`Leave`). Leaving a lock the thread does not
    /// hold does nothing, where FPC's behaviour is undefined.
    pub fn leave(&self) {
        let me = std::thread::current().id();
        let mut owner = super::lock(&self.owner);
        if owner.thread != Some(me) {
            return;
        }
        owner.depth -= 1;
        if owner.depth == 0 {
            owner.thread = None;
            self.released.notify_one();
        }
    }
}
