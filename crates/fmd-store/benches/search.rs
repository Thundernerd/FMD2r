//! Discover's search and facets over a 100k-title list (docs/tickets/T26-discover-list-update.md,
//! acceptance: search under 100 ms on a laptop; not gated in CI).
//!
//! Run with `cargo bench -p fmd-store --bench search`.

// A bench is test code (CODING_STANDARDS.md); clippy only exempts `#[test]` fns.
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::time::{Duration, Instant};

use fmd_store::{ListsDb, MangaListing, PageRequest, SearchFilters};

const TITLES: usize = 100_000;
const GENRES: [&str; 12] = [
    "Action",
    "Adventure",
    "Comedy",
    "Drama",
    "Fantasy",
    "Horror",
    "Isekai",
    "Mystery",
    "Romance",
    "School Life",
    "Shounen",
    "Slice of Life",
];
const WORDS: [&str; 16] = [
    "dragon", "piece", "sword", "academy", "hero", "demon", "king", "love", "night", "shadow",
    "star", "blade", "world", "magic", "girl", "tower",
];

fn listing(i: usize) -> MangaListing {
    let word = |n: usize| WORDS[(i / n) % WORDS.len()];
    let genres: Vec<&str> = (0..3)
        .map(|g| GENRES[(i * 7 + g * 5) % GENRES.len()])
        .collect();
    MangaListing {
        link: format!("/manga/{i}"),
        title: format!("{} {} {} {i}", word(1), word(3), word(11)),
        alttitles: format!("{} no {}", word(5), word(13)),
        authors: format!("Author {}", i % 997),
        genres: genres.join(", "),
        status: (i % 4).to_string(),
        summary: "A long enough summary to make the rows realistic. ".repeat(4),
        numchapter: (i % 300) as u32,
        added_jdn: 2_460_000 + (i % 1000) as i64,
        ..MangaListing::default()
    }
}

/// Prints the median of `runs` timings of `f`.
fn time(name: &str, runs: usize, mut f: impl FnMut()) {
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    times.sort();
    println!(
        "{name:<48} median {:>8.2?}  max {:>8.2?}",
        times[runs / 2],
        times[runs - 1]
    );
}

fn main() {
    let dir = tempfile::tempdir().unwrap();
    let db = ListsDb::open(dir.path().join("lists.db")).unwrap();
    let rows: Vec<MangaListing> = (0..TITLES).map(listing).collect();
    let start = Instant::now();
    db.masterlist().replace_module("site", &rows).unwrap();
    db.masterlist()
        .replace_module("other", rows.iter().take(1000))
        .unwrap();
    println!("import of {TITLES} titles: {:.2?}", start.elapsed());

    let repo = db.masterlist();
    let page = PageRequest {
        offset: 0,
        limit: 50,
    };
    let site = SearchFilters {
        module_ids: vec!["site".into()],
        ..SearchFilters::default()
    };
    let filtered = SearchFilters {
        include_genres: vec!["Action".into()],
        exclude_genres: vec!["Romance".into()],
        status: Some("1".into()),
        ..site.clone()
    };
    let runs = 21;
    time("search: everything, first page", runs, || {
        repo.search("", &SearchFilters::default(), page).unwrap();
    });
    time("search: one module, first page", runs, || {
        repo.search("", &site, page).unwrap();
    });
    time("search: one module, page 1000", runs, || {
        let deep = PageRequest {
            offset: 50_000,
            limit: 50,
        };
        repo.search("", &site, deep).unwrap();
    });
    time("search: 'dragon pie' in one module", runs, || {
        repo.search("dragon pie", &site, page).unwrap();
    });
    time("search: 'dr' (short prefix) in one module", runs, || {
        repo.search("dr", &site, page).unwrap();
    });
    time("search: genres + status in one module", runs, || {
        repo.search("", &filtered, page).unwrap();
    });
    time("search: 'sword' + genres + status", runs, || {
        repo.search("sword", &filtered, page).unwrap();
    });
    time("facets: one module", runs, || {
        repo.facets("", &site).unwrap();
    });
    time("facets: 'dragon' in one module", runs, || {
        repo.facets("dragon", &site).unwrap();
    });
}
