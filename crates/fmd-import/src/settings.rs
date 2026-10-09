//! `settings.json` → application settings (T18's model, `fmd_core::settings`).
//!
//! Format: a `TJSONIniFile` (baseunits/FMDOptions.pas:242): one object per section, one value per
//! key. The keys, their meaning and FMD2's defaults are in `TMainForm.LoadOptions`
//! (mangadownloader/forms/frmMain.pas:5803-5980); the language is read at :6896-6897. Each key is
//! applied as its own settings update, so an invalid value skips only that key.

use std::path::Path;
use std::sync::Arc;

use fmd_core::settings::{SettingsError, SettingsService};
use fmd_lua::crypto::decrypt_string;
use fmd_store::AppDb;
use serde_json::{Map, Value};

use crate::ImportOptions;
use crate::error::ImportError;
use crate::fmd2::{json_bool, json_int, read_json};
use crate::paths::translate;
use crate::report::{ImportReport, SkipReason, Unmapped};

const SOURCE: &str = "settings.json";

/// How an FMD2 value becomes an FMD2r one.
#[derive(Clone, Copy)]
enum Conv {
    Bool,
    Int,
    Str,
    /// A path, rewritten by the path maps.
    Path,
    /// An item index into these FMD2r enum values.
    Index(&'static [&'static str]),
    /// `cbOptionProxyType` text `HTTP`/`SOCKS4`/`SOCKS5` (mangadownloader/forms/frmMain.lfm:3559-3563).
    ProxyType,
    /// A port typed into a text field; empty means none.
    Port,
    /// Stored with `EncryptString` (read with `DecryptString`, mangadownloader/forms/frmMain.pas:5878-5879).
    Encrypted,
}

/// `(section, key, FMD2r setting path, conversion)`, in `LoadOptions` order.
const MAP: &[(&str, &str, &str, Conv)] = &[
    (
        "general",
        "AddAsStopped",
        "general.add_as_stopped",
        Conv::Bool,
    ),
    ("view", "LoadMangaCover", "general.load_covers", Conv::Bool),
    ("languages", "Selected", "general.language", Conv::Str),
    (
        "connections",
        "NumberOfTasks",
        "connections.max_parallel_tasks",
        Conv::Int,
    ),
    (
        "connections",
        "NumberOfThreadsPerTask",
        "connections.threads_per_task",
        Conv::Int,
    ),
    ("connections", "Retry", "connections.retry_count", Conv::Int),
    (
        "connections",
        "NumberOfAutoRetryFailedTask",
        "connections.auto_retry_failed_tasks",
        Conv::Int,
    ),
    (
        "connections",
        "AlwaysStartFromFailedChapters",
        "connections.always_start_from_failed_chapters",
        Conv::Bool,
    ),
    (
        "connections",
        "MaxFavoriteThreads",
        "connections.max_favorite_threads",
        Conv::Int,
    ),
    (
        "connections",
        "MaxUpdateListThreads",
        "connections.max_update_list_threads",
        Conv::Int,
    ),
    (
        "connections",
        "ConnectionTimeout",
        "connections.timeout_secs",
        Conv::Int,
    ),
    (
        "connections",
        "DefaultUserAgent",
        "connections.user_agent",
        Conv::Str,
    ),
    (
        "connections",
        "UseProxy",
        "connections.proxy.enabled",
        Conv::Bool,
    ),
    (
        "connections",
        "ProxyType",
        "connections.proxy.type",
        Conv::ProxyType,
    ),
    ("connections", "Host", "connections.proxy.host", Conv::Str),
    ("connections", "Port", "connections.proxy.port", Conv::Port),
    (
        "connections",
        "User",
        "connections.proxy.username",
        Conv::Encrypted,
    ),
    (
        "connections",
        "Pass",
        "connections.proxy.password",
        Conv::Encrypted,
    ),
    ("saveto", "SaveTo", "saveto.default_dir", Conv::Path),
    ("saveto", "PDFQuality", "output.pdf_quality", Conv::Int),
    // rgOptionCompress None/ZIP/CBZ/PDF/EPUB (mangadownloader/forms/frmMain.lfm:3912-3918).
    (
        "saveto",
        "Compress",
        "output.format",
        Conv::Index(&["folder", "zip", "cbz", "pdf", "epub"]),
    ),
    (
        "saveto",
        "ChangeUnicodeCharacter",
        "saveto.replace_unicode",
        Conv::Bool,
    ),
    (
        "saveto",
        "ChangeUnicodeCharacterStr",
        "saveto.replace_unicode_with",
        Conv::Str,
    ),
    (
        "saveto",
        "RemoveMangaNameFromChapter",
        "saveto.remove_manga_name_from_chapter",
        Conv::Bool,
    ),
    (
        "saveto",
        "GenerateMangaFolder",
        "saveto.generate_manga_folder",
        Conv::Bool,
    ),
    (
        "saveto",
        "MangaCustomRename",
        "saveto.manga_rename",
        Conv::Str,
    ),
    (
        "saveto",
        "GenerateChapterFolder",
        "saveto.generate_chapter_folder",
        Conv::Bool,
    ),
    (
        "saveto",
        "ChapterCustomRename",
        "saveto.chapter_rename",
        Conv::Str,
    ),
    (
        "saveto",
        "ConvertDigitVolume",
        "saveto.convert_digit_volume",
        Conv::Bool,
    ),
    (
        "saveto",
        "DigitVolumeLength",
        "saveto.digit_volume_length",
        Conv::Int,
    ),
    (
        "saveto",
        "ConvertDigitChapter",
        "saveto.convert_digit_chapter",
        Conv::Bool,
    ),
    (
        "saveto",
        "DigitChapterLength",
        "saveto.digit_chapter_length",
        Conv::Int,
    ),
    (
        "saveto",
        "FilenameCustomRename",
        "saveto.filename_rename",
        Conv::Str,
    ),
    ("saveto", "PNGSaveAsJPEG", "images.png_to_jpeg", Conv::Bool),
    // cbWebPSaveAs items (mangadownloader/forms/frmMain.lfm:4363-4367).
    (
        "saveto",
        "ConvertWebP",
        "images.webp_save_as",
        Conv::Index(&["webp", "png", "jpeg"]),
    ),
    // cbPNGCompressionLevel items (mangadownloader/forms/frmMain.lfm:4395-4400).
    (
        "saveto",
        "PNGCompressionLevel",
        "images.png_compression",
        Conv::Index(&["none", "fastest", "default", "maximum"]),
    ),
    ("saveto", "JPEGQuality", "images.jpeg_quality", Conv::Int),
    (
        "imagemagick",
        "ImageMagickEnabled",
        "images.imagemagick.enabled",
        Conv::Bool,
    ),
    (
        "imagemagick",
        "ImageMagickSaveAs",
        "images.imagemagick.save_as",
        Conv::Str,
    ),
    (
        "imagemagick",
        "ImageMagickCompression",
        "images.imagemagick.compression",
        Conv::Str,
    ),
    (
        "imagemagick",
        "ImageMagickQuality",
        "images.imagemagick.quality",
        Conv::Int,
    ),
    (
        "update",
        "AutoCheckFavStartup",
        "favorites.check_at_startup",
        Conv::Bool,
    ),
    (
        "update",
        "AutoCheckFavInterval",
        "favorites.check_on_interval",
        Conv::Bool,
    ),
    (
        "update",
        "AutoCheckFavIntervalMinutes",
        "favorites.check_interval_minutes",
        Conv::Int,
    ),
    (
        "update",
        "NewMangaTime",
        "update_lists.new_manga_days",
        Conv::Int,
    ),
    (
        "update",
        "AutoCheckFavAutoDownload",
        "favorites.auto_download",
        Conv::Bool,
    ),
    (
        "update",
        "AutoCheckFavAutoRemoveCompletedManga",
        "favorites.remove_completed",
        Conv::Bool,
    ),
    (
        "update",
        "UpdateListNoMangaInfo",
        "update_lists.no_manga_info",
        Conv::Bool,
    ),
    (
        "update",
        "UpdateListRemoveDuplicateLocalData",
        "update_lists.remove_duplicate_local_data",
        Conv::Bool,
    ),
];

/// A value as text, for the report.
fn display(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The FMD2r value for `value`, or why there is none.
fn convert(
    conv: Conv,
    value: &Value,
    opts: &ImportOptions,
    report: &mut ImportReport,
) -> Result<Value, String> {
    let not = |what: &str| format!("{} is not {what}", display(value));
    Ok(match conv {
        Conv::Bool => Value::Bool(json_bool(Some(value)).ok_or_else(|| not("a boolean"))?),
        Conv::Int => Value::from(json_int(Some(value)).ok_or_else(|| not("an integer"))?),
        Conv::Str => Value::String(display(value)),
        Conv::Path => Value::String(translate(&opts.path_maps, &display(value), report)),
        Conv::Index(items) => {
            let i = json_int(Some(value)).ok_or_else(|| not("an integer"))?;
            let item = usize::try_from(i)
                .ok()
                .and_then(|i| items.get(i))
                .ok_or_else(|| not("a known item index"))?;
            Value::String((*item).to_string())
        }
        Conv::ProxyType => Value::String(display(value).trim().to_ascii_lowercase()),
        Conv::Port => match display(value).trim() {
            "" => Value::Null,
            port => Value::from(port.parse::<u16>().map_err(|_| not("a port"))?),
        },
        Conv::Encrypted => Value::String(
            String::from_utf8(decrypt_string(display(value).as_bytes()))
                .map_err(|_| "does not decrypt to UTF-8 text".to_string())?,
        ),
    })
}

/// `{"a": {"b": value}}` for the path `a.b`.
fn patch(path: &str, value: Value) -> Value {
    path.rsplit('.').fold(value, |inner, key| {
        Value::Object(Map::from_iter([(key.to_string(), inner)]))
    })
}

pub(crate) fn import(
    path: &Path,
    db: &AppDb,
    opts: &ImportOptions,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    let Some(sections): Option<Map<String, Value>> = read_json(path)? else {
        return Ok(());
    };
    report.settings.found = true;

    let service = if opts.dry_run {
        // Validate against a throwaway copy of the current settings.
        let current = SettingsService::load(db.clone())?.get();
        let scratch = SettingsService::load(AppDb::open(":memory:")?)?;
        scratch.update(serde_json::to_value(&*current).map_err(SettingsError::from)?)?;
        scratch
    } else {
        SettingsService::load(db.clone())?
    };
    let mut unmapped = Vec::new();
    for (section, keys) in &sections {
        let Some(keys) = keys.as_object() else {
            continue;
        };
        for (key, value) in keys {
            let item = format!("{section}/{key}");
            let mapping = MAP.iter().find(|(s, k, _, _)| {
                s.eq_ignore_ascii_case(section) && k.eq_ignore_ascii_case(key)
            });
            let Some(&(_, _, setting, conv)) = mapping else {
                unmapped.push(Unmapped {
                    source: SOURCE.into(),
                    key: item,
                    value: display(value),
                });
                continue;
            };
            let value = match convert(conv, value, opts, report) {
                Ok(value) => value,
                Err(why) => {
                    report.settings.skip(item, SkipReason::Invalid(why));
                    continue;
                }
            };
            let before = service.get();
            match service.update(patch(setting, value)) {
                Ok(after) if Arc::ptr_eq(&before, &after) => {
                    report.settings.skip(item, SkipReason::AlreadyExists);
                }
                Ok(_) => report.settings.imported += 1,
                Err(e @ SettingsError::Invalid(_)) => {
                    report
                        .settings
                        .skip(item, SkipReason::Invalid(e.to_string()));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    unmapped.sort_by(|a, b| a.key.cmp(&b.key));
    report.unmapped.extend(unmapped);
    Ok(())
}
