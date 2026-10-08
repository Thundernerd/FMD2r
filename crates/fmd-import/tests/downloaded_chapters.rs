// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::{App, FavoriteRow, Fmd2, text};
use fmd_import::SkipReason;

const MODULE: &str = "46e0c618a19748d6af150c2f198f5360";

#[test]
fn downloaded_chapters_split_fmd2s_lowercased_module_id_and_link_key() {
    let fmd2 = Fmd2::new();
    // The key is `LowerCase(AModuleID+ALink)` (baseunits/DownloadedChaptersDB.pas:53, :65).
    fmd2.favorites(&[FavoriteRow {
        link: "/Manga/Berserk",
        ..FavoriteRow::default()
    }]);
    fmd2.downloaded_chapters(&[
        (
            "46e0c618a19748d6af150c2f198f5360/manga/berserk",
            text(&["/Ch/1", "/Ch/2"]),
        ),
        // A manga that is neither a favorite nor a task: the module id is FMD2's 32 hex digits.
        (
            "7bda2905b61c49d1976777e9f2356361https://site.test/m/x",
            text(&["https://site.test/c/1"]),
        ),
    ]);
    let app = App::new();

    let report = app.import(&fmd2);

    assert!(report.downloaded_chapters.found);
    assert_eq!(report.downloaded_chapters.imported, 2);
    let dl = app.db.downloaded_chapters();
    assert!(dl.contains(MODULE, "/Manga/Berserk", "/Ch/1").unwrap());
    assert!(dl.contains(MODULE, "/Manga/Berserk", "/Ch/2").unwrap());
    assert!(
        dl.contains(
            "7bda2905b61c49d1976777e9f2356361",
            "https://site.test/m/x",
            "https://site.test/c/1"
        )
        .unwrap()
    );
}

#[test]
fn a_key_without_a_recognisable_module_id_is_skipped_as_invalid() {
    let fmd2 = Fmd2::new();
    fmd2.downloaded_chapters(&[("mymodule/manga/x", text(&["/c/1"]))]);
    let app = App::new();

    let report = app.import(&fmd2);

    assert_eq!(report.downloaded_chapters.imported, 0);
    assert_eq!(report.downloaded_chapters.skipped.len(), 1);
    assert_eq!(
        report.downloaded_chapters.skipped[0].item,
        "mymodule/manga/x"
    );
    assert!(matches!(
        report.downloaded_chapters.skipped[0].reason,
        SkipReason::Invalid(_)
    ));
}
