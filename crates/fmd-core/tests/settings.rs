// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_core::settings::{
    OutputFormat, SettingsError, SettingsService, SymbolMode, WebpSaveAs, XPathBackend,
};
use fmd_store::AppDb;
use serde_json::json;

fn open_db() -> (tempfile::TempDir, AppDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    (dir, db)
}

#[test]
fn fresh_db_returns_fmd2_defaults() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db).unwrap();
    let s = service.get();

    // DEFAULT_MANGA_CUSTOMRENAME etc. (baseunits/FMDOptions.pas:24-26).
    assert_eq!(s.saveto.manga_rename, "%MANGA%");
    assert_eq!(s.saveto.chapter_rename, "%CHAPTER%");
    assert_eq!(s.saveto.filename_rename, "%FILENAME%");
    // DEFAULT_PATH (baseunits/FMDOptions.pas:283).
    assert_eq!(s.saveto.default_dir, "downloads");
    // DigitVolumeLength 2, DigitChapterLength 3 (mangadownloader/forms/frmMain.pas:5907-5911).
    assert!(s.saveto.convert_digit_volume);
    assert_eq!(s.saveto.digit_volume_length, 2);
    assert_eq!(s.saveto.digit_chapter_length, 3);
    // OptionChangeUnicodeCharacterStr (baseunits/FMDOptions.pas:109).
    assert!(!s.saveto.replace_unicode);
    assert_eq!(s.saveto.replace_unicode_with, "_");
    assert_eq!(s.saveto.illegal_chars, SymbolMode::Posix);

    // OptionMaxParallel, OptionMaxThreads, OptionMaxRetry, OptionConnectionTimeout
    // (baseunits/FMDOptions.pas:129-135).
    assert_eq!(s.connections.max_parallel_tasks, 1);
    assert_eq!(s.connections.threads_per_task, 1);
    assert_eq!(s.connections.retry_count, 5);
    assert_eq!(s.connections.timeout_secs, 30);
    assert!(s.connections.user_agent.starts_with("Mozilla/5.0"));

    // PDFQuality 100, Compress 0 = None (mangadownloader/forms/frmMain.pas:5888-5889).
    assert_eq!(s.output.format, OutputFormat::Folder);
    assert_eq!(s.output.pdf_quality, 100);
    // OptionWebPSaveAs 1 = PNG, OptionJPEGQuality 80 (baseunits/FMDOptions.pas:124-126).
    assert_eq!(s.images.webp_save_as, WebpSaveAs::Png);
    assert_eq!(s.images.jpeg_quality, 80);
    // ImageMagickQuality 75 (mangadownloader/forms/frmMain.pas:5942).
    assert_eq!(s.images.imagemagick.quality, 75);

    // AutoCheckFavStartup true, AutoCheckFavIntervalMinutes 60, AutoCheckFavAutoDownload false
    // (mangadownloader/forms/frmMain.pas:5946-5954).
    assert!(s.favorites.check_at_startup);
    assert_eq!(s.favorites.check_interval_minutes, 60);
    assert!(!s.favorites.auto_download);

    // GitHub repo (dist/config.json:8-15).
    assert_eq!(s.module_updater.repo_owner, "dazedcat19");
    assert_eq!(s.module_updater.repo_name, "FMD2");
    assert_eq!(s.module_updater.repo_ref, "master");
    assert_eq!(s.module_updater.repo_path, "lua");

    // The native XPath engine, at parity with FMD2's on the differential corpus (T35).
    assert_eq!(s.xpath.backend, XPathBackend::Native);

    // Covers are revalidated weekly and capped at 256 MiB.
    assert_eq!(s.covers.revalidate_after_hours, 168);
    assert_eq!(s.covers.cache_size_mb, 256);
}

#[test]
fn invalid_update_is_rejected_and_nothing_is_persisted() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db.clone()).unwrap();

    // Negative threads, together with a valid change that must not be applied either.
    let err = service
        .update(json!({
            "connections": { "threads_per_task": -1 },
            "general": { "language": "nl" },
        }))
        .unwrap_err();
    assert!(matches!(err, SettingsError::Invalid(_)), "{err:?}");

    // Zero threads deserialises but is out of range (MinValue 1,
    // mangadownloader/forms/frmMain.lfm:3703-3704).
    let err = service
        .update(json!({ "connections": { "threads_per_task": 0 } }))
        .unwrap_err();
    assert!(matches!(err, SettingsError::Invalid(_)), "{err:?}");

    // Not an output format (rgOptionCompress items, mangadownloader/forms/frmMain.lfm:3912-3918).
    let err = service
        .update(json!({ "output": { "format": "rar" } }))
        .unwrap_err();
    assert!(matches!(err, SettingsError::Invalid(_)), "{err:?}");

    assert_eq!(service.get().connections.threads_per_task, 1);
    assert_eq!(service.get().general.language, "en");
    let reloaded = SettingsService::load(db).unwrap();
    assert_eq!(*reloaded.get(), *service.get());
    assert_eq!(reloaded.get().general.language, "en");
}

#[test]
fn partial_patch_updates_only_given_keys_and_notifies_once() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db.clone()).unwrap();
    let mut rx = service.subscribe();
    assert!(!rx.has_changed().unwrap());

    let updated = service
        .update(json!({
            "connections": { "max_parallel_tasks": 4, "proxy": { "host": "proxy.lan" } },
            "saveto": { "manga_rename": "%WEBSITE% - %MANGA%" },
        }))
        .unwrap();

    let defaults = SettingsService::load(open_db().1).unwrap().get();
    let mut expected = (*defaults).clone();
    expected.connections.max_parallel_tasks = 4;
    expected.connections.proxy.host = "proxy.lan".into();
    expected.saveto.manga_rename = "%WEBSITE% - %MANGA%".into();
    assert_eq!(*updated, expected);
    assert_eq!(*service.get(), expected);

    // One notification carrying the whole patch.
    assert!(rx.has_changed().unwrap());
    assert_eq!(**rx.borrow_and_update(), expected);
    assert!(!rx.has_changed().unwrap());

    // Persisted.
    assert_eq!(*SettingsService::load(db).unwrap().get(), expected);

    // A patch that changes nothing does not notify.
    service
        .update(json!({ "connections": { "max_parallel_tasks": 4 } }))
        .unwrap();
    assert!(!rx.has_changed().unwrap());
}

#[test]
fn patch_null_resets_to_default_and_unknown_keys_are_rejected() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db).unwrap();
    service
        .update(json!({ "connections": { "timeout_secs": 90 } }))
        .unwrap();
    service
        .update(json!({ "connections": { "timeout_secs": null } }))
        .unwrap();
    // OptionConnectionTimeout (baseunits/FMDOptions.pas:129).
    assert_eq!(service.get().connections.timeout_secs, 30);

    let err = service
        .update(json!({ "connections": { "max_threads": 2 } }))
        .unwrap_err();
    assert!(
        matches!(&err, SettingsError::Invalid(e) if e[0].field == "connections.max_threads"),
        "{err:?}"
    );
}

#[test]
fn blank_templates_and_user_agent_fall_back_to_fmd2_defaults() {
    let (_dir, db) = open_db();
    let service = SettingsService::load(db).unwrap();
    // mangadownloader/forms/frmMain.pas:5894-5917, :6279-6285.
    let s = service
        .update(json!({
            "saveto": { "chapter_rename": "  ", "filename_rename": "" },
            "connections": { "user_agent": " " },
        }))
        .unwrap();
    assert_eq!(s.saveto.chapter_rename, "%CHAPTER%");
    assert_eq!(s.saveto.filename_rename, "%FILENAME%");
    assert!(s.connections.user_agent.starts_with("Mozilla/5.0"));
}

#[test]
fn stored_groups_are_forward_compatible() {
    let (_dir, db) = open_db();
    // A group written by an older build (fields missing) and one written by a newer build
    // (a field this build doesn't know).
    db.settings()
        .set("output", &json!({ "format": "cbz" }))
        .unwrap();
    db.settings()
        .set(
            "favorites",
            &json!({ "check_interval_minutes": 15, "from_the_future": [1, 2] }),
        )
        .unwrap();

    let service = SettingsService::load(db.clone()).unwrap();
    assert_eq!(service.get().output.format, OutputFormat::Cbz);
    assert_eq!(service.get().output.pdf_quality, 100);
    assert_eq!(service.get().favorites.check_interval_minutes, 15);

    service
        .update(json!({ "favorites": { "auto_download": true } }))
        .unwrap();
    let stored: serde_json::Value = db.settings().get("favorites").unwrap().unwrap();
    assert_eq!(stored["from_the_future"], json!([1, 2]));
    assert_eq!(stored["auto_download"], json!(true));
    assert_eq!(stored["check_interval_minutes"], json!(15));

    // Lossless: what was stored loads back as the same settings.
    assert_eq!(*SettingsService::load(db).unwrap().get(), *service.get());
}

#[test]
fn a_bad_stored_value_falls_back_to_its_default_without_losing_the_rest() {
    let (_dir, db) = open_db();
    // A newer build's output format and a value of the wrong type.
    db.settings()
        .set("output", &json!({ "format": "cbr", "pdf_quality": 50 }))
        .unwrap();
    db.settings()
        .set(
            "connections",
            &json!({ "max_parallel_tasks": 3, "timeout_secs": "slow" }),
        )
        .unwrap();

    let s = SettingsService::load(db).unwrap().get();
    assert_eq!(s.output.format, OutputFormat::Folder);
    assert_eq!(s.output.pdf_quality, 50);
    assert_eq!(s.connections.max_parallel_tasks, 3);
    assert_eq!(s.connections.timeout_secs, 30);
}
