//! A clock whose sleeps return at once, moving its time forward instead.

use std::sync::Mutex;
use std::time::Duration;

use fmd_core::download::Clock;

#[derive(Default)]
pub struct FakeClock {
    now: Mutex<Duration>,
}

impl FakeClock {
    /// Fake time gone by since the clock was made.
    pub fn elapsed(&self) -> Duration {
        *self.now.lock().unwrap()
    }

    pub fn advance(&self, by: Duration) {
        *self.now.lock().unwrap() += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        self.elapsed()
    }

    /// Several sleepers end at the latest of their deadlines, as they would on a real clock.
    fn sleep_until(&self, deadline: Duration) {
        let mut now = self.now.lock().unwrap();
        *now = (*now).max(deadline);
    }
}
