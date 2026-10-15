//! The total download speed limit (issue #309), on a fake clock.
//!
//! No FMD2 behaviour: FMD2 has no speed limit. Expected values follow from the limit itself:
//! N KB/s lets N KB through per second, 1 KB being 1024 bytes.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use common::clock::FakeClock;
use fmd_core::download::SpeedLimiter;

const KB: u64 = 1024;

fn limiter(clock: &Arc<FakeClock>, kbps: &Arc<AtomicU32>) -> Arc<SpeedLimiter> {
    let kbps = kbps.clone();
    Arc::new(SpeedLimiter::new(clock.clone(), move || {
        kbps.load(Ordering::SeqCst)
    }))
}

fn never() -> bool {
    false
}

#[test]
fn taking_four_seconds_worth_from_one_caller_takes_four_seconds() {
    let clock = Arc::new(FakeClock::default());
    let limiter = limiter(&clock, &Arc::new(AtomicU32::new(10)));

    for _ in 0..4 {
        assert!(limiter.take(10 * KB, &never));
    }

    assert_eq!(clock.elapsed(), Duration::from_secs(4));
}

#[test]
fn taking_four_seconds_worth_from_several_callers_takes_four_seconds() {
    let clock = Arc::new(FakeClock::default());
    let limiter = limiter(&clock, &Arc::new(AtomicU32::new(10)));

    std::thread::scope(|scope| {
        for _ in 0..4 {
            let limiter = limiter.clone();
            scope.spawn(move || assert!(limiter.take(10 * KB, &never)));
        }
    });

    assert_eq!(clock.elapsed(), Duration::from_secs(4));
}

#[test]
fn a_limit_of_zero_never_waits() {
    let clock = Arc::new(FakeClock::default());
    let limiter = limiter(&clock, &Arc::new(AtomicU32::new(0)));

    for _ in 0..4 {
        assert!(limiter.take(10_000 * KB, &never));
    }

    assert_eq!(clock.elapsed(), Duration::ZERO);
}

#[test]
fn a_changed_limit_applies_to_the_next_take() {
    let clock = Arc::new(FakeClock::default());
    let kbps = Arc::new(AtomicU32::new(10));
    let limiter = limiter(&clock, &kbps);

    assert!(limiter.take(10 * KB, &never));
    assert_eq!(clock.elapsed(), Duration::from_secs(1));

    kbps.store(20, Ordering::SeqCst);
    assert!(limiter.take(10 * KB, &never));
    assert_eq!(clock.elapsed(), Duration::from_millis(1500));

    kbps.store(0, Ordering::SeqCst);
    assert!(limiter.take(10 * KB, &never));
    assert_eq!(clock.elapsed(), Duration::from_millis(1500));
}

#[test]
fn a_take_already_waiting_picks_up_a_changed_limit() {
    let clock = Arc::new(FakeClock::default());
    let at = clock.clone();
    // 1 KB/s for the first two seconds, 100 KB/s after.
    let limiter = SpeedLimiter::new(clock.clone(), move || {
        if at.elapsed() < Duration::from_secs(2) {
            1
        } else {
            100
        }
    });

    assert!(limiter.take(102 * KB, &never));

    // 2 KB in the first two seconds, the other 100 KB in one more: not 102 seconds.
    assert_eq!(clock.elapsed(), Duration::from_secs(3));
}

#[test]
fn a_cancelled_take_stops_waiting() {
    let clock = Arc::new(FakeClock::default());
    let limiter = limiter(&clock, &Arc::new(AtomicU32::new(1)));

    // An hour's worth at 1 KB/s, cut short once ten seconds have gone by.
    let cancel = || clock.elapsed() >= Duration::from_secs(10);
    assert!(!limiter.take(3600 * KB, &cancel));

    assert!(clock.elapsed() < Duration::from_secs(12));
}

#[test]
fn time_spent_idle_is_not_saved_up_for_a_burst() {
    let clock = Arc::new(FakeClock::default());
    let limiter = limiter(&clock, &Arc::new(AtomicU32::new(10)));

    clock.advance(Duration::from_secs(60));
    for _ in 0..2 {
        assert!(limiter.take(10 * KB, &never));
    }

    assert_eq!(clock.elapsed(), Duration::from_secs(62));
}
