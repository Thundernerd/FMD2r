// Test helpers outside `#[test]` fns are not covered by clippy.toml's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use std::cmp::Ordering;

use fmd_pack::natural_cmp;

fn sorted(items: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    v.sort_by(|a, b| natural_cmp(a, b));
    v
}

/// naturalsortunit.pas:246-261: digit runs compare by value.
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

/// naturalsortunit.pas:262-267 and the "Logical sort" note at :74-84: equal values with
/// more leading zeros sort first.
#[test]
fn more_leading_zeros_sort_first() {
    assert_eq!(
        sorted(&["01", "1", "001", "0001"]),
        ["0001", "001", "01", "1"]
    );
}

/// naturalsortunit.pas:300-308: equal so far, the shorter string sorts first.
#[test]
fn shorter_string_sorts_first_when_otherwise_equal() {
    assert_eq!(natural_cmp("1", "1a"), Ordering::Less);
    assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
}

/// naturalsortunit.pas:236-245: leading spaces before a chunk are skipped.
#[test]
fn leading_spaces_are_skipped() {
    assert_eq!(natural_cmp(" 2", "10"), Ordering::Less);
}

/// naturalsortunit.pas:271-292: text runs (up to the next digit) are compared whole with
/// `strcoll` (:282), which FMD2 runs in the C locale on Linux, i.e. bytewise.
#[test]
fn text_runs_compare_bytewise() {
    assert_eq!(
        sorted(&["b2", "B1", "a10", "A9"]),
        ["A9", "B1", "a10", "b2"]
    );
    assert_eq!(natural_cmp("a1.jpg", "A1.jpg"), Ordering::Greater);
    assert_eq!(natural_cmp("a1", "a 1"), Ordering::Less);
}
