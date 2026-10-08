//! FMD2's natural sort for page files (baseunits/naturalsortunit.pas).

use std::cmp::Ordering;

/// Compares like `UTF8LogicalCompareText` (baseunits/naturalsortunit.pas:311-321), which FMD2
/// uses to order the files it packs (baseunits/uPacker.pas:279, baseunits/uBaseUnit.pas:2651).
///
/// Digit runs compare by value, and on a tie the run with more leading characters (zeros or
/// skipped spaces) sorts first. Text runs compare bytewise: on Linux FMD2 calls `strcoll`
/// (:274-289) without setting a locale, so it collates like `strcmp`. When everything compares
/// equal, the shorter string sorts first (:300-308).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (s1, s2) = (a.as_bytes(), b.as_bytes());
    let (mut p1, mut p2) = (0, 0);
    let mut result = Ordering::Equal;
    if !s1.is_empty() && !s2.is_empty() {
        while p1 < s1.len() && p2 < s2.len() {
            let (mut len1, mut len2) = (0, 0);
            while s1.get(p1) == Some(&b' ') {
                p1 += 1;
                len1 += 1;
            }
            while s2.get(p2) == Some(&b' ') {
                p2 += 1;
                len2 += 1;
            }
            let digit1 = s1.get(p1).is_some_and(u8::is_ascii_digit);
            let digit2 = s2.get(p2).is_some_and(u8::is_ascii_digit);
            if digit1 && digit2 {
                let n1 = run(s1, p1, |c| c.is_ascii_digit());
                let n2 = run(s2, p2, |c| c.is_ascii_digit());
                result = cmp_digits(&s1[p1..p1 + n1], &s2[p2..p2 + n2])
                    // `Result := -Sign(Len1 - Len2)` (:262-267).
                    .then_with(|| (len2 + n2).cmp(&(len1 + n1)));
                p1 += n1;
                p2 += n2;
            } else {
                let n1 = run(s1, p1, |c| !c.is_ascii_digit());
                let n2 = run(s2, p2, |c| !c.is_ascii_digit());
                result = s1[p1..p1 + n1].cmp(&s2[p2..p2 + n2]);
                p1 += n1;
                p2 += n2;
            }
            if result != Ordering::Equal {
                break;
            }
        }
    }
    result.then_with(|| s1.len().cmp(&s2.len()))
}

/// Length of the run starting at `from` whose bytes satisfy `f`.
fn run(s: &[u8], from: usize, f: impl Fn(&u8) -> bool) -> usize {
    s[from..].iter().take_while(|c| f(c)).count()
}

/// Compares two ASCII digit runs by numeric value, without overflow.
fn cmp_digits(a: &[u8], b: &[u8]) -> Ordering {
    let strip = |s: &[u8]| -> usize { s.iter().take_while(|&&c| c == b'0').count() };
    let (a, b) = (&a[strip(a)..], &b[strip(b)..]);
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}
