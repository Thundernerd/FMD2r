// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::{App, Fmd2};
use fmd_core::settings::{OutputFormat, PngCompression, ProxyType, SettingsService, WebpSaveAs};
use fmd_import::SkipReason;
use serde_json::json;

/// `settings.json` is a `TJSONIniFile` (baseunits/FMDOptions.pas:242): sections as objects,
/// written by `SaveOptions` and read by `LoadOptions` (mangadownloader/forms/frmMain.pas:5803-5980).
fn settings_json() -> String {
    json!({
        "general": {
            "OneInstanceOnly": false,
            "AddAsStopped": true,
            // Module IDs joined by commas (mangadownloader/forms/frmMain.pas:5990-6004).
            "MangaListSelect": "a,b"
        },
        "darkmode": { "mode": 1 },
        "languages": { "Selected": "nl" },
        "view": { "LoadMangaCover": false },
        "connections": {
            "NumberOfTasks": 4,
            "NumberOfThreadsPerTask": 8,
            "Retry": -1,
            "NumberOfAutoRetryFailedTask": 3,
            "AlwaysStartFromFailedChapters": false,
            "MaxFavoriteThreads": 2,
            "MaxUpdateListThreads": 5,
            "ConnectionTimeout": 45,
            "DefaultUserAgent": "My UA",
            "UseProxy": true,
            "ProxyType": "SOCKS5",
            "Host": "127.0.0.1",
            "Port": "9050",
            // EncryptString('hunter2') (baseunits/uBaseUnit.pas:1559-1573; read at frmMain.pas:5878).
            "User": "pcjhSQpguA==",
            "Pass": "pcjhSQpguA=="
        },
        "saveto": {
            "SaveTo": "/data/manga",
            "Compress": 2,
            "PDFQuality": 90,
            "ChangeUnicodeCharacter": true,
            "ChangeUnicodeCharacterStr": "-",
            "RemoveMangaNameFromChapter": true,
            "GenerateMangaFolder": false,
            "MangaCustomRename": "%MANGA% (%WEBSITE%)",
            "GenerateChapterFolder": false,
            "ChapterCustomRename": "%NUMBERING% - %CHAPTER%",
            "ConvertDigitVolume": false,
            "DigitVolumeLength": 3,
            "ConvertDigitChapter": false,
            "DigitChapterLength": 4,
            "FilenameCustomRename": "%FILENAME%_x",
            "PNGSaveAsJPEG": true,
            "ConvertWebP": 2,
            "PNGCompressionLevel": 3,
            "JPEGQuality": 70
        },
        "imagemagick": {
            "ImageMagickEnabled": true,
            "ImageMagickSaveAs": "PNG",
            "ImageMagickCompression": "Zip",
            "ImageMagickQuality": 60
        },
        "update": {
            "AutoCheckLatestVersion": false,
            "AutoCheckFavStartup": false,
            "AutoCheckFavInterval": false,
            "AutoCheckFavIntervalMinutes": 30,
            "NewMangaTime": 7,
            "AutoCheckFavAutoDownload": true,
            "AutoCheckFavAutoRemoveCompletedManga": true,
            "UpdateListNoMangaInfo": true,
            "UpdateListRemoveDuplicateLocalData": true
        }
    })
    .to_string()
}

#[test]
fn known_settings_map_to_the_settings_model() {
    let fmd2 = Fmd2::new();
    fmd2.file("settings.json", &settings_json());
    let app = App::new();

    let report = app.import(&fmd2);

    assert!(report.settings.found);
    assert!(report.settings.skipped.is_empty(), "{:?}", report.settings);
    let s = SettingsService::load(app.db.clone()).unwrap().get();

    assert!(s.general.add_as_stopped);
    assert!(!s.general.load_covers);
    assert_eq!(s.general.language, "nl");
    assert_eq!(s.general.selected_websites, ["a", "b"]);

    let c = &s.connections;
    assert_eq!(
        (c.max_parallel_tasks, c.threads_per_task, c.retry_count),
        (4, 8, -1)
    );
    assert_eq!(c.auto_retry_failed_tasks, 3);
    assert!(!c.always_start_from_failed_chapters);
    assert_eq!((c.max_favorite_threads, c.max_update_list_threads), (2, 5));
    assert_eq!(c.timeout_secs, 45);
    assert_eq!(c.user_agent, "My UA");
    assert!(c.proxy.enabled);
    assert_eq!(c.proxy.kind, ProxyType::Socks5);
    assert_eq!(c.proxy.host, "127.0.0.1");
    assert_eq!(c.proxy.port, Some(9050));
    assert_eq!(c.proxy.username, "hunter2");
    assert_eq!(c.proxy.password, "hunter2");

    let t = &s.saveto;
    assert_eq!(t.default_dir, "/data/manga");
    assert!(t.replace_unicode);
    assert_eq!(t.replace_unicode_with, "-");
    assert!(t.remove_manga_name_from_chapter);
    assert!(!t.generate_manga_folder);
    assert_eq!(t.manga_rename, "%MANGA% (%WEBSITE%)");
    assert!(!t.generate_chapter_folder);
    assert_eq!(t.chapter_rename, "%NUMBERING% - %CHAPTER%");
    assert!(!t.convert_digit_volume);
    assert_eq!(t.digit_volume_length, 3);
    assert!(!t.convert_digit_chapter);
    assert_eq!(t.digit_chapter_length, 4);
    assert_eq!(t.filename_rename, "%FILENAME%_x");

    // rgOptionCompress None/ZIP/CBZ/PDF/EPUB (mangadownloader/forms/frmMain.lfm:3912-3918).
    assert_eq!(s.output.format, OutputFormat::Cbz);
    assert_eq!(s.output.pdf_quality, 90);

    assert!(s.images.png_to_jpeg);
    assert_eq!(s.images.webp_save_as, WebpSaveAs::Jpeg);
    assert_eq!(s.images.png_compression, PngCompression::Maximum);
    assert_eq!(s.images.jpeg_quality, 70);
    assert!(s.images.imagemagick.enabled);
    assert_eq!(s.images.imagemagick.save_as, "PNG");
    assert_eq!(s.images.imagemagick.compression, "Zip");
    assert_eq!(s.images.imagemagick.quality, 60);

    let f = &s.favorites;
    assert!(!f.check_at_startup && !f.check_on_interval);
    assert_eq!(f.check_interval_minutes, 30);
    assert!(f.auto_download && f.remove_completed);
    assert!(s.update_lists.no_manga_info && s.update_lists.remove_duplicate_local_data);
    assert_eq!(s.update_lists.new_manga_days, 7);
}

#[test]
fn unmapped_settings_are_listed_in_the_report() {
    let fmd2 = Fmd2::new();
    fmd2.file("settings.json", &settings_json());
    let app = App::new();

    let report = app.import(&fmd2);

    let unmapped: Vec<_> = report
        .unmapped
        .iter()
        .filter(|u| u.source == "settings.json")
        .map(|u| (u.key.as_str(), u.value.as_str()))
        .collect();
    assert_eq!(
        unmapped,
        [
            ("darkmode/mode", "1"),
            ("general/OneInstanceOnly", "false"),
            ("update/AutoCheckLatestVersion", "false"),
        ]
    );
}

#[test]
fn an_invalid_setting_is_skipped_and_the_rest_imported() {
    let fmd2 = Fmd2::new();
    fmd2.file(
        "settings.json",
        &json!({ "connections": { "ConnectionTimeout": 0, "NumberOfTasks": 3 } }).to_string(),
    );
    let app = App::new();

    let report = app.import(&fmd2);

    assert_eq!(report.settings.imported, 1);
    assert_eq!(report.settings.skipped.len(), 1);
    assert_eq!(
        report.settings.skipped[0].item,
        "connections/ConnectionTimeout"
    );
    assert!(matches!(
        report.settings.skipped[0].reason,
        SkipReason::Invalid(_)
    ));
    let s = SettingsService::load(app.db.clone()).unwrap().get();
    assert_eq!(s.connections.max_parallel_tasks, 3);
    assert_eq!(s.connections.timeout_secs, 30);
}
