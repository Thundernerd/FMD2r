// Test helpers outside `#[test]` fns are not covered by clippy.toml's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use std::cmp::Ordering;

use fmd_pack::natural_cmp;

fn sorted(items: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    v.sort_by(|a, b| natural_cmp(a, b));
    v
}

/// naturalsortunit.pas:218-237: digit runs compare by value.
#[test]
fn numbers_compare_by_value() {
    assert_eq!(
        sorted(&[
            "10.jpg", "2.jpg", "1.jpg", "page10", "page9", "x1y10", "x1y2"
        ]),
        [
            "1.jpg", "2.jpg", "10.jpg", "page9", "page10", "x1y2", "x1y10"
        ]
    );
}

/// naturalsortunit.pas:230-235 and the "Logical sort" note at :74-80: equal values with
/// more leading zeros sort first.
#[test]
fn more_leading_zeros_sort_first() {
    assert_eq!(
        sorted(&["01", "1", "001", "0001"]),
        ["0001", "001", "01", "1"]
    );
}

/// naturalsortunit.pas:268-275: equal so far, the shorter string sorts first.
#[test]
fn shorter_string_sorts_first_when_otherwise_equal() {
    assert_eq!(natural_cmp("1", "1a"), Ordering::Less);
    assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
}

/// naturalsortunit.pas:209-218: leading spaces before a chunk are skipped.
#[test]
fn leading_spaces_are_skipped() {
    assert_eq!(natural_cmp(" 2", "10"), Ordering::Less);
}

/// naturalsortunit.pas:240-258: text runs (up to the next digit) are compared whole,
/// ignoring case like `StrCmpLogicalW` (:283).
#[test]
fn text_runs_compare_ignoring_case() {
    assert_eq!(
        sorted(&["b2", "B1", "a10", "A9"]),
        ["A9", "a10", "B1", "b2"]
    );
    assert_eq!(natural_cmp("a1", "a 1"), Ordering::Less);
}
