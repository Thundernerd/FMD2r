// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::{App, every_source};
use fmd_core::settings::SettingsService;
use fmd_import::{ImportOptions, ImportReport, SourceReport};

fn counts(s: &SourceReport) -> (usize, usize) {
    (s.imported, s.already_existing())
}

fn all_counts(r: &ImportReport) -> [(usize, usize); 6] {
    [
        counts(&r.tasks),
        counts(&r.favorites),
        counts(&r.downloaded_chapters),
        counts(&r.module_settings),
        counts(&r.accounts),
        counts(&r.settings),
    ]
}

#[test]
fn a_second_import_duplicates_nothing_and_reports_everything_as_existing() {
    let fmd2 = every_source();
    let app = App::new();

    let first = app.import(&fmd2);
    let second = app.import(&fmd2);

    assert_eq!(
        all_counts(&first),
        [(2, 0), (1, 0), (1, 0), (1, 0), (1, 0), (2, 0)]
    );
    assert_eq!(
        all_counts(&second),
        [(0, 2), (0, 1), (0, 1), (0, 1), (0, 1), (0, 2)]
    );
    assert_eq!(app.db.tasks().list().unwrap().len(), 2);
    assert_eq!(app.db.favorites().list().unwrap().len(), 1);
    assert_eq!(
        app.db
            .downloaded_chapters()
            .list_for("46e0c618a19748d6af150c2f198f5360", "/manga/berserk")
            .unwrap(),
        ["/ch/1"]
    );
}

#[test]
fn a_dry_run_reports_what_would_be_imported_and_writes_nothing() {
    let fmd2 = every_source();
    let app = App::new();
    let dry = ImportOptions {
        dry_run: true,
        ..ImportOptions::default()
    };

    let report = app.import_with(&fmd2, &dry);

    assert!(report.dry_run);
    assert_eq!(
        all_counts(&report),
        [(2, 0), (1, 0), (1, 0), (1, 0), (1, 0), (2, 0)]
    );
    assert!(app.db.tasks().list().unwrap().is_empty());
    assert!(app.db.favorites().list().unwrap().is_empty());
    let dl = app.db.downloaded_chapters();
    assert!(
        !dl.contains("46e0c618a19748d6af150c2f198f5360", "/manga/other", "/ch/9")
            .unwrap()
    );
    assert!(
        !dl.contains(
            "46e0c618a19748d6af150c2f198f5360",
            "/manga/berserk",
            "/ch/1"
        )
        .unwrap()
    );
    assert!(
        app.db
            .module_settings()
            .get("d07c9c2425764da8ba056505f57cf40c")
            .unwrap()
            .is_none()
    );
    assert!(
        app.db
            .accounts(&app.cipher)
            .get("d07c9c2425764da8ba056505f57cf40c")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        SettingsService::load(app.db.clone())
            .unwrap()
            .get()
            .connections
            .max_parallel_tasks,
        1
    );

    // The real import afterwards matches what the dry run promised.
    let real = app.import(&fmd2);
    assert_eq!(all_counts(&real), all_counts(&report));
}
