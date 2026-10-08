// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::{App, DownloadRow, FavoriteRow, Fmd2};
use fmd_core::settings::SettingsService;
use fmd_import::{ImportOptions, PathMap};

fn mapped(maps: &[&str]) -> ImportOptions {
    ImportOptions {
        path_maps: maps.iter().map(|m| m.parse().unwrap()).collect(),
        ..ImportOptions::default()
    }
}

fn task_paths(app: &App) -> Vec<String> {
    app.db
        .tasks()
        .list()
        .unwrap()
        .into_iter()
        .map(|t| t.save_to)
        .collect()
}

#[test]
fn map_path_rewrites_windows_save_to_paths_of_tasks_favorites_and_settings() {
    let fmd2 = Fmd2::new();
    fmd2.downloads(&[
        DownloadRow::default(),
        DownloadRow {
            order: 1,
            saveto: "C:\\Manga\\Guts",
            dateadded: "2024-01-01 00:00:00.000",
            ..DownloadRow::default()
        },
    ]);
    fmd2.favorites(&[FavoriteRow {
        saveto: "c:\\MANGA\\Berserk\\",
        ..FavoriteRow::default()
    }]);
    fmd2.file("settings.json", r#"{"saveto":{"SaveTo":"C:\\Manga"}}"#);
    let app = App::new();

    let report = app.import_with(&fmd2, &mapped(&["C:\\Manga=/data/manga"]));

    assert_eq!(task_paths(&app), ["/manga/Berserk", "/data/manga/Guts"]);
    let fav = &app.db.favorites().list().unwrap()[0];
    // Windows paths compare case-insensitively.
    assert_eq!(fav.save_to, "/data/manga/Berserk/");
    let settings = SettingsService::load(app.db.clone()).unwrap().get();
    assert_eq!(settings.saveto.default_dir, "/data/manga");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn the_longest_matching_prefix_wins_and_only_at_a_path_boundary() {
    let fmd2 = Fmd2::new();
    fmd2.downloads(&[
        DownloadRow {
            saveto: "D:\\Manga\\Done\\Berserk",
            dateadded: "2024-01-01 00:00:00.000",
            ..DownloadRow::default()
        },
        DownloadRow {
            order: 1,
            saveto: "D:\\Manga\\Berserk",
            dateadded: "2024-01-01 00:00:01.000",
            ..DownloadRow::default()
        },
        DownloadRow {
            order: 2,
            saveto: "D:\\MangaX\\Berserk",
            dateadded: "2024-01-01 00:00:02.000",
            ..DownloadRow::default()
        },
    ]);
    let app = App::new();

    let report = app.import_with(&fmd2, &mapped(&["D:\\Manga=/m", "D:\\Manga\\Done\\=/done"]));

    assert_eq!(
        task_paths(&app),
        ["/done/Berserk", "/m/Berserk", "D:\\MangaX\\Berserk"]
    );
    // The Windows path no map covered is imported as is, with a warning.
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("D:\\MangaX\\Berserk"));
}

#[test]
fn a_path_map_parses_from_from_equals_to() {
    let map: PathMap = "C:\\Manga=/data/manga".parse().unwrap();
    assert_eq!(map.from, "C:\\Manga");
    assert_eq!(map.to, "/data/manga");
    assert!("no-equals-sign".parse::<PathMap>().is_err());
}
