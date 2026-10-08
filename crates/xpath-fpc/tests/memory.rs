//! Rough leak checks: every handle has a free function, so a parse/eval/free loop must not grow the process.
//!
//! A separate test binary, so no other test allocates while the resident set size is measured.

// A test crate: panicking is how a check fails, including in helpers outside #[test] functions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;

const HTML: &str = r#"<ul><li><a href="/1" title="one">a</a></li><li> b </li><p>x<p>y</ul>"#;

/// Resident set size in KiB, from `/proc/self/statm` (second field, in pages).
fn rss_kib() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: u64 = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4
}

/// Exercises every handle-returning call once.
fn round_trip() {
    let doc = Doc::parse(HTML);
    let items = doc.eval("//li");
    assert_eq!(items.count(), 2);
    let first = items.get(1);
    assert_eq!(first.string(), "a");
    assert_eq!(
        doc.eval_in("a", Some(&first), false).attribute("href"),
        "/1"
    );
    assert_eq!(
        doc.eval_in("li a", None, true).outer_html(),
        r#"<a href="/1" title="one">a</a>"#
    );
    assert_eq!(first.inner_html(), r#"<a href="/1" title="one">a</a>"#);
    assert_eq!(first.inner_text(), "a");
    let json = doc.eval(r#"json('{"a":[1,2],"b":"x"}')"#);
    assert_eq!(json.property("b").string(), "x");
    assert_eq!(doc.eval("//li[").count(), 0);
    assert!(!take(unsafe { fx_last_error() }).is_empty());
}

/// Runs `round_trip` `n` times and returns how much the process grew, in KiB.
fn growth_kib(n: usize, each: impl Fn()) -> u64 {
    for _ in 0..n / 10 {
        each();
    }
    let before = rss_kib();
    for _ in 0..n {
        each();
    }
    rss_kib().saturating_sub(before)
}

#[test]
fn parse_eval_free_loops_do_not_grow() {
    // A leaked document (engine, parser, tree) is tens of KiB, so 5 000 iterations would add over 50 MiB.
    let growth = growth_kib(5_000, round_trip);
    assert!(growth < 4 * 1024, "grew by {growth} KiB");

    // Short-lived host threads: FPC allocates each one's threadvars on its first call; fx_thread_exit frees them.
    let thread = || {
        round_trip();
        unsafe { fx_thread_exit() };
        // A thread may call in again afterwards.
        round_trip();
        unsafe { fx_thread_exit() };
    };
    let growth = growth_kib(300, || std::thread::spawn(thread).join().unwrap());
    assert!(growth < 4 * 1024, "threads grew by {growth} KiB");
}
