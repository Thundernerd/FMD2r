//! The list titles' stored cover links (docs/tickets/T70-discover-cover-thumbnails.md, "Seams
//! under test"): they outlive a list update and an FMD2-DB import of the same module.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_store::{CoverLink, CoverSource, ListsDb, MangaListing};

fn listing(link: &str, title: &str) -> MangaListing {
    MangaListing {
        link: link.into(),
        title: title.into(),
        ..MangaListing::default()
    }
}

fn website(url: Option<&str>) -> CoverLink {
    CoverLink {
        url: url.map(str::to_owned),
        large_url: None,
        source: CoverSource::Website,
        series_id: None,
        checked_at: 1_000,
    }
}

fn open() -> (tempfile::TempDir, ListsDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = ListsDb::open(dir.path().join("lists.db")).unwrap();
    db.masterlist()
        .replace_module("m", [listing("/1", "One"), listing("/2", "Two")])
        .unwrap();
    (dir, db)
}

#[test]
fn a_stored_cover_link_and_a_known_missing_cover_read_back() {
    let (_dir, db) = open();
    let covers = db.cover_links();
    covers
        .put("m", "/1", &website(Some("https://site.test/1.jpg")))
        .unwrap();
    covers.put("m", "/2", &website(None)).unwrap();

    assert_eq!(
        covers.get("m", "/1").unwrap(),
        Some(website(Some("https://site.test/1.jpg")))
    );
    assert_eq!(covers.get("m", "/2").unwrap(), Some(website(None)));
    assert_eq!(covers.get("m", "/3").unwrap(), None);
    assert_eq!(covers.get("other", "/1").unwrap(), None);
}

#[test]
fn cover_links_survive_a_list_update_and_an_fmd2_db_import() {
    let (_dir, db) = open();
    let stored = website(Some("https://site.test/1.jpg"));
    db.cover_links().put("m", "/1", &stored).unwrap();

    // A list update adds the new titles.
    db.masterlist()
        .insert_new("m", [listing("/1", "One"), listing("/3", "Three")])
        .unwrap();
    assert_eq!(
        db.cover_links().get("m", "/1").unwrap(),
        Some(stored.clone())
    );

    // An FMD2-DB import replaces the whole list.
    db.masterlist()
        .replace_module("m", [listing("/1", "One (renamed)")])
        .unwrap();
    assert_eq!(db.cover_links().get("m", "/1").unwrap(), Some(stored));
}
