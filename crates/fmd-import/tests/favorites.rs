// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::{App, FavoriteRow, Fmd2, text};

const MODULE: &str = "46e0c618a19748d6af150c2f198f5360";

#[test]
fn a_favorite_keeps_its_fields_dates_and_downloaded_chapters() {
    let fmd2 = Fmd2::new();
    fmd2.favorites(&[FavoriteRow {
        enabled: false,
        status: "1",
        currentchapter: "42",
        downloadedchapterlist: text(&["/ch/1", "/ch/2"]),
        ..FavoriteRow::default()
    }]);
    let app = App::new();

    let report = app.import(&fmd2);

    assert!(report.favorites.found);
    assert_eq!(report.favorites.imported, 1);
    let fav = app
        .db
        .favorites()
        .find(MODULE, "/manga/berserk")
        .unwrap()
        .unwrap();
    assert_eq!(fav.title, "Berserk");
    assert_eq!(fav.status, "1");
    assert_eq!(fav.current_chapter, 42);
    assert_eq!(fav.save_to, "/manga/Berserk");
    assert!(!fav.enabled);
    assert_eq!(fav.date_added, 1_672_628_645_006);
    assert_eq!(fav.date_last_checked, Some(1_675_209_600_000));
    assert_eq!(fav.date_last_updated, Some(1_675_296_000_000));

    let dl = app.db.downloaded_chapters();
    assert!(dl.contains(MODULE, "/manga/berserk", "/ch/1").unwrap());
    assert!(dl.contains(MODULE, "/manga/berserk", "/ch/2").unwrap());
    assert!(!dl.contains(MODULE, "/manga/berserk", "/ch/3").unwrap());
}

#[test]
fn favorites_keep_fmd2s_order() {
    let fmd2 = Fmd2::new();
    fmd2.favorites(&[
        FavoriteRow {
            id: "b",
            order: 1,
            link: "/b",
            title: "B",
            ..FavoriteRow::default()
        },
        FavoriteRow {
            id: "a",
            order: 0,
            link: "/a",
            title: "A",
            ..FavoriteRow::default()
        },
    ]);
    let app = App::new();

    app.import(&fmd2);

    let titles: Vec<_> = app
        .db
        .favorites()
        .list()
        .unwrap()
        .into_iter()
        .map(|f| f.title)
        .collect();
    assert_eq!(titles, ["A", "B"]);
}

#[test]
fn a_missing_source_is_reported_as_not_found() {
    let report = App::new().import(&Fmd2::new());
    assert!(!report.favorites.found);
    assert!(!report.tasks.found);
    assert_eq!(report.favorites.imported, 0);
}
