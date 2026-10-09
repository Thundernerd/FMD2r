// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

//! Named download destinations (T74): `saveto.destinations`, one of them the default.

use fmd_core::settings::{Destination, SettingsError, SettingsService};
use fmd_store::AppDb;
use serde_json::json;

fn open_db() -> (tempfile::TempDir, AppDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    (dir, db)
}

fn destination(name: &str, path: &str, default: bool) -> Destination {
    Destination {
        name: name.into(),
        path: path.into(),
        default,
    }
}

/// The fields an update rejected.
fn rejected(result: Result<impl std::fmt::Debug, SettingsError>) -> Vec<(String, String)> {
    match result {
        Err(SettingsError::Invalid(errors)) => {
            errors.into_iter().map(|e| (e.field, e.reason)).collect()
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn a_fresh_install_has_one_default_destination() {
    let (_dir, db) = open_db();
    let s = SettingsService::load(db).unwrap().get();
    // DEFAULT_PATH (baseunits/FMDOptions.pas:283).
    assert_eq!(
        s.saveto.destinations,
        vec![destination("Downloads", "downloads", true)]
    );
    assert_eq!(s.saveto.default_dir, "downloads");
}

#[test]
fn the_stored_download_folder_becomes_the_default_destination() {
    let (_dir, db) = open_db();
    // What a build before destinations stored.
    db.settings()
        .set(
            "saveto",
            &json!({ "default_dir": "/data/manga", "generate_manga_folder": false }),
        )
        .unwrap();

    let service = SettingsService::load(db.clone()).unwrap();
    let s = service.get();
    assert_eq!(
        s.saveto.destinations,
        vec![destination("Downloads", "/data/manga", true)]
    );
    assert_eq!(s.saveto.default_dir, "/data/manga");
    assert!(!s.saveto.generate_manga_folder);

    // The migration is stored, so it runs once.
    let stored: serde_json::Value = db.settings().get("saveto").unwrap().unwrap();
    assert_eq!(
        stored["destinations"],
        json!([{ "name": "Downloads", "path": "/data/manga", "default": true }])
    );
    assert_eq!(*SettingsService::load(db).unwrap().get(), *s);
}

#[test]
fn several_destinations_can_be_set_up_and_the_default_is_the_download_folder() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db.clone()).unwrap();
    let s = service
        .update(json!({ "saveto": { "destinations": [
            { "name": "Manga", "path": "/data/manga" },
            { "name": "Manhwa", "path": "/data/manhwa", "default": true },
        ] } }))
        .unwrap();
    assert_eq!(
        s.saveto.destinations,
        vec![
            destination("Manga", "/data/manga", false),
            destination("Manhwa", "/data/manhwa", true),
        ]
    );
    // `default_dir` follows the default, for API clients that only know it.
    assert_eq!(s.saveto.default_dir, "/data/manhwa");
    assert_eq!(*SettingsService::load(db).unwrap().get(), *s);
}

#[test]
fn setting_the_download_folder_moves_the_default_destination() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db).unwrap();
    service
        .update(json!({ "saveto": { "destinations": [
            { "name": "Manga", "path": "/data/manga", "default": true },
            { "name": "Manhwa", "path": "/data/manhwa" },
        ] } }))
        .unwrap();

    let s = service
        .update(json!({ "saveto": { "default_dir": "/mnt/manga" } }))
        .unwrap();
    assert_eq!(
        s.saveto.destinations,
        vec![
            destination("Manga", "/mnt/manga", true),
            destination("Manhwa", "/data/manhwa", false),
        ]
    );
    assert_eq!(s.saveto.default_dir, "/mnt/manga");
}

#[test]
fn names_must_be_unique_and_not_empty_and_paths_not_empty() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db).unwrap();
    let errors = rejected(service.update(json!({ "saveto": { "destinations": [
        { "name": "Manga", "path": "/data/manga", "default": true },
        { "name": "manga ", "path": "/data/other" },
        { "name": " ", "path": "/data/blank" },
        { "name": "No path", "path": "" },
    ] } })));
    let fields: Vec<&str> = errors.iter().map(|(f, _)| f.as_str()).collect();
    assert_eq!(
        fields,
        [
            "saveto.destinations.1.name",
            "saveto.destinations.2.name",
            "saveto.destinations.3.path",
        ],
        "{errors:?}"
    );
    // Nothing changed.
    assert_eq!(service.get().saveto.destinations.len(), 1);
}

#[test]
fn the_default_destination_cannot_be_removed() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db).unwrap();
    service
        .update(json!({ "saveto": { "destinations": [
            { "name": "Manga", "path": "/data/manga", "default": true },
            { "name": "Manhwa", "path": "/data/manhwa" },
        ] } }))
        .unwrap();

    let errors = rejected(service.update(json!({ "saveto": { "destinations": [
        { "name": "Manhwa", "path": "/data/manhwa" },
    ] } })));
    assert_eq!(
        errors,
        [(
            "saveto.destinations".to_string(),
            "the default destination cannot be removed".to_string()
        )]
    );
    let errors = rejected(service.update(json!({ "saveto": { "destinations": [] } })));
    assert_eq!(errors[0].0, "saveto.destinations");

    // Making another one the default first, it can.
    let s = service
        .update(json!({ "saveto": { "destinations": [
            { "name": "Manhwa", "path": "/data/manhwa", "default": true },
        ] } }))
        .unwrap();
    assert_eq!(s.saveto.default_dir, "/data/manhwa");

    let errors = rejected(service.update(json!({ "saveto": { "destinations": [
        { "name": "Manhwa", "path": "/data/manhwa", "default": true },
        { "name": "Manga", "path": "/data/manga", "default": true },
    ] } })));
    assert_eq!(errors[0].0, "saveto.destinations");
}
