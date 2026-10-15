//! The total download speed limit (`connections.max_download_kbps`). No FMD2 setting: FMD2
//! downloads as fast as it can.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Time for the [`SpeedLimiter`]; tests swap in a fake one.
pub trait Clock: Send + Sync {
    /// Time since a fixed origin.
    fn now(&self) -> Duration;
    /// Returns once [`now`](Clock::now) has reached `deadline`.
    fn sleep_until(&self, deadline: Duration);
}

/// The system's monotonic clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

fn origin() -> Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(Instant::now)
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        origin().elapsed()
    }

    fn sleep_until(&self, deadline: Duration) {
        std::thread::sleep(deadline.saturating_sub(self.now()));
    }
}

/// The longest a [`SpeedLimiter::take`] sleeps before it looks at the limit and its `cancelled`
/// again, so a changed limit or a stopped task is noticed within this time.
const SLICE: Duration = Duration::from_millis(250);

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// One limit shared by all its callers: at N KB/s (1 KB = 1024 bytes) they get N KB a second
/// between them. A [`take`](SpeedLimiter::take) is paid for after the bytes came in, so callers
/// burst by up to one take each. Time spent idle is not saved up.
pub struct SpeedLimiter {
    clock: Arc<dyn Clock>,
    kbps: Box<dyn Fn() -> u32 + Send + Sync>,
    state: Mutex<State>,
}

/// Bytes are counted in nano-bytes (bytes × 10⁹), so that paying for `t` ns at `r` bytes/s is
/// exactly `t × r`.
#[derive(Default)]
struct State {
    /// All bytes taken so far.
    taken: u128,
    /// The part of `taken` paid for, up to `at`.
    paid: u128,
    at: Duration,
    /// Bytes per second since `at`; 0 for no limit.
    rate: u128,
}

impl State {
    /// Pays for the time from `at` to `now` at the current rate, then switches to `rate`.
    fn advance(&mut self, now: Duration, rate: u128) {
        let elapsed = now.saturating_sub(self.at).as_nanos();
        self.paid = match self.rate {
            0 => self.taken,
            r => self
                .paid
                .saturating_add(elapsed.saturating_mul(r))
                .min(self.taken),
        };
        self.at = self.at.max(now);
        self.rate = rate;
        if rate == 0 {
            self.paid = self.taken;
        }
    }
}

impl SpeedLimiter {
    /// `kbps` is read on every take, and while one waits; 0 means no limit.
    pub fn new(clock: Arc<dyn Clock>, kbps: impl Fn() -> u32 + Send + Sync + 'static) -> Self {
        SpeedLimiter {
            clock,
            kbps: Box::new(kbps),
            state: Mutex::default(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn rate(&self) -> u128 {
        u128::from((self.kbps)()) * 1024
    }

    /// Waits until `bytes` fit under the limit, behind the bytes taken before them. Returns
    /// false when `cancelled` cut the wait short.
    pub fn take(&self, bytes: u64, cancelled: &dyn Fn() -> bool) -> bool {
        let target = {
            let mut state = self.state();
            state.advance(self.clock.now(), self.rate());
            state.taken += u128::from(bytes) * NANOS_PER_SEC;
            state.taken
        };
        loop {
            let wait = {
                let mut state = self.state();
                let now = self.clock.now();
                state.advance(now, self.rate());
                if state.paid >= target {
                    return true;
                }
                let owed = target - state.paid;
                let nanos = owed.div_ceil(state.rate);
                let wait = Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX));
                now + wait.min(SLICE)
            };
            if cancelled() {
                return false;
            }
            self.clock.sleep_until(wait);
        }
    }
}
