use fmd_store::{ListsDb, MangaListing, PageRequest, SearchFilters};

fn listing(link: &str, title: &str, alttitles: &str, genres: &str) -> MangaListing {
    MangaListing {
        link: link.into(),
        title: title.into(),
        alttitles: alttitles.into(),
        genres: genres.into(),
        ..MangaListing::default()
    }
}

fn links(db: &ListsDb, query: &str, filters: &SearchFilters) -> Vec<String> {
    db.masterlist()
        .search(query, filters, PageRequest { offset: 0, limit: 100 })
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.listing.link)
        .collect()
}

fn open() -> (tempfile::TempDir, ListsDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = ListsDb::open(dir.path().join("lists.db")).unwrap();
    db.masterlist()
        .replace_module(
            "m",
            [
                listing("/1", "One Piece", "", "Action, Adventure, Comedy"),
                listing("/2", "Wan Pisu", "One Piece; ワンピース", "Action, Adventure"),
                listing("/3", "Naruto", "", "Action, Romance"),
                listing("/4", "Piece of Cake", "", "Comedy, Romance"),
            ],
        )
        .unwrap();
    (dir, db)
}

#[test]
fn search_finds_by_title_and_alttitle() {
    let (_dir, db) = open();
    let none = SearchFilters::default();
    assert_eq!(links(&db, "one piece", &none), ["/1", "/2"]);
    assert_eq!(links(&db, "ワンピース", &none), ["/2"]);
    assert_eq!(links(&db, "", &none).len(), 4);
    assert_eq!(db.masterlist().count(Some("m")).unwrap(), 4);
    assert_eq!(db.masterlist().count(Some("other")).unwrap(), 0);
}

#[test]
fn search_supports_prefix_queries() {
    let (_dir, db) = open();
    assert_eq!(links(&db, "one pi", &SearchFilters::default()), ["/1", "/2"]);
    assert_eq!(links(&db, "naru", &SearchFilters::default()), ["/3"]);
}

#[test]
fn search_tolerates_fts_syntax_in_queries() {
    let (_dir, db) = open();
    assert!(links(&db, "\"one\" AND -piece* NEAR(", &SearchFilters::default()).is_empty());
    assert_eq!(links(&db, "\"", &SearchFilters::default()).len(), 4);
}

#[test]
fn genre_include_and_exclude_filters() {
    let (_dir, db) = open();
    let include = SearchFilters {
        include_genres: vec!["action".into(), "adventure".into()],
        ..SearchFilters::default()
    };
    assert_eq!(links(&db, "", &include), ["/1", "/2"]);

    let exclude = SearchFilters {
        exclude_genres: vec!["Comedy".into()],
        ..SearchFilters::default()
    };
    assert_eq!(links(&db, "", &exclude), ["/3", "/2"]);

    let both = SearchFilters {
        include_genres: vec!["Romance".into()],
        exclude_genres: vec!["Comedy".into()],
        ..SearchFilters::default()
    };
    assert_eq!(links(&db, "", &both), ["/3"]);
    assert_eq!(links(&db, "piece", &exclude), ["/2"]);
}

#[test]
fn pagination_is_stable_and_reports_total() {
    let (_dir, db) = open();
    let repo = db.masterlist();
    let page = |offset| {
        repo.search("", &SearchFilters::default(), PageRequest { offset, limit: 3 })
            .unwrap()
    };
    let first = page(0);
    let second = page(3);
    assert_eq!(first.total, 4);
    assert_eq!(second.total, 4);
    let all: Vec<_> = first
        .entries
        .iter()
        .chain(&second.entries)
        .map(|e| e.listing.title.as_str())
        .collect();
    assert_eq!(all, ["Naruto", "One Piece", "Piece of Cake", "Wan Pisu"]);
    assert_eq!(page(0).entries, first.entries);
}

#[test]
fn fts_stays_in_sync_on_upsert_and_replace() {
    let (_dir, db) = open();
    let repo = db.masterlist();
    let none = SearchFilters::default();

    repo.upsert("m", &listing("/3", "Boruto", "", "Action")).unwrap();
    assert!(links(&db, "naruto", &none).is_empty());
    assert_eq!(links(&db, "boruto", &none), ["/3"]);

    repo.upsert("other", &listing("/9", "Bleach", "", "Action")).unwrap();
    assert_eq!(links(&db, "bleach", &none), ["/9"]);
    let only_m = SearchFilters {
        module_ids: vec!["m".into()],
        ..SearchFilters::default()
    };
    assert!(links(&db, "bleach", &only_m).is_empty());

    repo.replace_module("m", [listing("/1", "One Piece", "", "")]).unwrap();
    assert!(links(&db, "boruto", &none).is_empty());
    assert_eq!(links(&db, "piece", &none), ["/1"]);
    assert_eq!(links(&db, "bleach", &none), ["/9"]);
    assert_eq!(repo.count(None).unwrap(), 2);
}
