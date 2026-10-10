//! How titles, names and links are compared: T71's matching rules
//! (docs/research/metadata-sources.md, "Matching"; `norm`, `title_keys`, `person_keys`,
//! `same_person` and `link_key` in docs/research/metadata-probe/probe.py).

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

/// A bracketed decoration such as "(Colored)", "[EN]" or "【Official】".
static BRACKETS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[\(\[\{（【][^\)\]\}）】]*[\)\]\}）】]").ok());
static NON_WORD: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[^\w]+").ok());
static NAME_SEPARATORS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[,;/、]| and ").ok());
static WEBTOONS_TITLE_NO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"webtoons\.com/.*[?&]title_no=(\d+)").ok());

/// How similar two names must be (`SequenceMatcher.ratio()`) to be the same person.
const SAME_PERSON_RATIO: f64 = 0.75;

/// `s` lower-cased (NFKC, then case-folded), without accents, apostrophes and punctuation, with
/// `&` as "and" and single spaces between words.
pub(crate) fn norm(s: &str) -> String {
    let folded = casefold(&s.nfkc().collect::<String>());
    let stripped: String = folded
        .nfkd()
        .filter(|c| canonical_combining_class(*c) == 0)
        .collect();
    let s = stripped.replace('&', " and ").replace(['’', '\''], "");
    let s = match NON_WORD.as_ref() {
        Some(re) => re.replace_all(&s, " ").into_owned(),
        None => s,
    };
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Python's `str.casefold` where it differs from lower-casing.
fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss").replace('ς', "σ")
}

fn strip_brackets(s: &str) -> String {
    match BRACKETS.as_ref() {
        Some(re) => re.replace_all(s, " ").into_owned(),
        None => s.to_owned(),
    }
}

/// The normalised title, plus the version without decorations when that differs.
pub(crate) fn title_keys(title: &str) -> Vec<String> {
    let mut keys = Vec::with_capacity(2);
    let full = norm(title);
    if !full.is_empty() {
        keys.push(full);
    }
    let bare = norm(&strip_brackets(title));
    if !bare.is_empty() && !keys.contains(&bare) {
        keys.push(bare);
    }
    keys
}

/// Each name's words sorted and joined, so family/given name order does not matter.
pub(crate) fn person_keys(names: &str) -> BTreeSet<String> {
    let parts: Vec<&str> = match NAME_SEPARATORS.as_ref() {
        Some(re) => re.split(names).collect(),
        None => vec![names],
    };
    parts
        .into_iter()
        .filter_map(|name| {
            let normalised = norm(&strip_brackets(name));
            let mut words: Vec<&str> = normalised.split_whitespace().collect();
            words.sort_unstable();
            (!words.is_empty()).then(|| words.concat())
        })
        .collect()
}

pub(crate) fn people_agree<'a>(
    a: impl IntoIterator<Item = &'a String>,
    b: &BTreeSet<String>,
) -> bool {
    a.into_iter().any(|x| b.iter().any(|y| same_person(x, y)))
}

/// Allows for romanisation differences (Hyun-woo / Hyeon-woo) and reordered syllables
/// (Soboro / Boroso).
fn same_person(x: &str, y: &str) -> bool {
    if x == y || ratio(x, y) >= SAME_PERSON_RATIO {
        return true;
    }
    let mut xs: Vec<char> = x.chars().collect();
    let mut ys: Vec<char> = y.chars().collect();
    xs.sort_unstable();
    ys.sort_unstable();
    xs.len() >= 4 && xs == ys
}

/// Python's `difflib.SequenceMatcher(None, x, y).ratio()`, without the junk heuristic (names
/// are far below its 200-character threshold).
fn ratio(x: &str, y: &str) -> f64 {
    let a: Vec<char> = x.chars().collect();
    let b: Vec<char> = y.chars().collect();
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    let matched = matching_chars(&a, &b);
    2.0 * matched as f64 / total as f64
}

/// Characters in `SequenceMatcher.get_matching_blocks`.
fn matching_chars(a: &[char], b: &[char]) -> usize {
    let mut matched = 0;
    let mut queue = vec![(0, a.len(), 0, b.len())];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest_match(a, b, alo, ahi, blo, bhi);
        if k == 0 {
            continue;
        }
        matched += k;
        if alo < i && blo < j {
            queue.push((alo, i, blo, j));
        }
        if i + k < ahi && j + k < bhi {
            queue.push((i + k, ahi, j + k, bhi));
        }
    }
    matched
}

/// `SequenceMatcher.find_longest_match` without junk, as `(start in a, start in b, length)`.
fn longest_match(
    a: &[char],
    b: &[char],
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> (usize, usize, usize) {
    let (mut besti, mut bestj, mut bestk) = (alo, blo, 0);
    // Run lengths ending at each position of b, for the previous position of a.
    let mut prev = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate().take(ahi).skip(alo) {
        let mut next = vec![0usize; b.len() + 1];
        for (j, cb) in b.iter().enumerate().take(bhi).skip(blo) {
            if ca == cb {
                let k = prev[j] + 1;
                next[j + 1] = k;
                if k > bestk {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestk = k;
                }
            }
        }
        prev = next;
    }
    (besti, bestj, bestk)
}

/// A key for links to sites with stable IDs: `webtoons:<title_no>`.
pub(crate) fn link_key(url: &str) -> Option<String> {
    let caps = WEBTOONS_TITLE_NO.as_ref()?.captures(url)?;
    Some(format!("webtoons:{}", caps.get(1)?.as_str()))
}
