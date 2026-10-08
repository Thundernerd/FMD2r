//! `GET/PATCH /api/settings` over the typed settings model (T18), and `POST /api/preview-rename`.

use axum::Json;
use axum::extract::State;
use fmd_core::settings::{
    DEFAULT_CHAPTER_CUSTOMRENAME, DEFAULT_FILENAME_CUSTOMRENAME, DEFAULT_MANGA_CUSTOMRENAME,
    SaveToSettings, Settings, SettingsError, SymbolMode,
};
use fmd_pack::{RenameContext, RenameOptions, custom_rename, page_file_name};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::{ApiError, AppState, Problem};

impl From<SettingsError> for ApiError {
    fn from(err: SettingsError) -> Self {
        match err {
            SettingsError::Invalid { field, reason } => ApiError::Invalid {
                field: Some(field),
                detail: reason,
            },
            SettingsError::UnknownKey(field) => ApiError::Invalid {
                detail: format!("unknown setting {field}"),
                field: Some(field),
            },
            SettingsError::Store(e) => ApiError::Store(e),
            SettingsError::Json(e) => ApiError::Internal(e.to_string()),
        }
    }
}

/// All settings.
#[utoipa::path(get, path = "/api/settings", tag = "settings", operation_id = "getSettings",
    responses((status = 200, body = Settings, description = "Every setting")))]
pub(crate) async fn get(State(state): State<AppState>) -> Json<Settings> {
    Json(state.settings.get().as_ref().clone())
}

/// Update settings with a JSON merge patch (RFC 7396) over [`Settings`]; `null` resets a setting
/// to its default. Nothing is stored unless the whole result is valid.
#[utoipa::path(patch, path = "/api/settings", tag = "settings", operation_id = "patchSettings",
    request_body(content = HashMap<String, Value>, content_type = "application/json"),
    responses(
        (status = 200, body = Settings, description = "The updated settings"),
        (status = 400, description = "The patch is not a JSON object", body = Problem),
        (status = 422, description = "A value is invalid or a setting unknown; `field` names it",
            body = Problem),
    ))]
pub(crate) async fn patch(
    State(state): State<AppState>,
    ApiJson(patch): ApiJson<Value>,
) -> Result<Json<Settings>, ApiError> {
    if !patch.is_object() {
        return Err(ApiError::BadRequest("expected a JSON object".into()));
    }
    let settings = state.settings.clone();
    let updated = crate::state::off_thread(move || settings.update(patch)).await??;
    Ok(Json(updated.as_ref().clone()))
}

/// The sample series a rename preview expands the templates for.
const SAMPLE_WEBSITE: &str = "MangaDex";
const SAMPLE_MANGA: &str = "Sample Manga";
const SAMPLE_AUTHOR: &str = "Sample Author";
const SAMPLE_ARTIST: &str = "Sample Artist";
const SAMPLE_CHAPTER: &str = "Vol. 1 Ch. 5";
/// FMD2 numbers chapters `%.4d` (mangadownloader/forms/frmMain.pas:2667-2675).
const SAMPLE_NUMBERING: &str = "0005";

/// The names the rename templates of a draft produce for a sample series.
#[derive(Debug, Serialize, ToSchema)]
pub struct RenamePreview {
    /// The manga folder name (`manga_rename`).
    pub manga: String,
    /// The chapter folder or archive name (`chapter_rename`).
    pub chapter: String,
    /// The first page's file name, without extension (`filename_rename`).
    pub filename: String,
}

/// Preview the rename templates of a (possibly unsaved) `saveto` group on a sample series, the
/// way downloads will name their folders and files.
#[utoipa::path(post, path = "/api/preview-rename", tag = "settings",
    operation_id = "previewRename",
    request_body(content = SaveToSettings, content_type = "application/json"),
    responses(
        (status = 200, body = RenamePreview, description = "The expanded names"),
        (status = 400, description = "Malformed body", body = Problem),
    ))]
pub(crate) async fn preview_rename(
    ApiJson(saveto): ApiJson<SaveToSettings>,
) -> Json<RenamePreview> {
    let opts = RenameOptions {
        symbols: match saveto.illegal_chars {
            SymbolMode::Posix => fmd_pack::SymbolMode::Posix,
            SymbolMode::Windows => fmd_pack::SymbolMode::Windows,
        },
        replace_unicode: saveto
            .replace_unicode
            .then(|| saveto.replace_unicode_with.clone()),
        pad_volume: digits(saveto.convert_digit_volume, saveto.digit_volume_length),
        pad_chapter: digits(saveto.convert_digit_chapter, saveto.digit_chapter_length),
    };
    let series = RenameContext {
        website: SAMPLE_WEBSITE,
        manga: SAMPLE_MANGA,
        author: SAMPLE_AUTHOR,
        artist: SAMPLE_ARTIST,
        ..RenameContext::default()
    };
    let chapter = RenameContext {
        chapter: SAMPLE_CHAPTER,
        numbering: SAMPLE_NUMBERING,
        ..series.clone()
    };
    // A blank template loads as its default (mangadownloader/forms/frmMain.pas:5894-5917), and
    // the settings service stores it that way.
    let template = |t: &str, default: &'static str| -> String {
        if t.trim().is_empty() { default } else { t }.to_owned()
    };
    Json(RenamePreview {
        manga: custom_rename(
            &template(&saveto.manga_rename, DEFAULT_MANGA_CUSTOMRENAME),
            &series,
            &opts,
        ),
        chapter: custom_rename(
            &template(&saveto.chapter_rename, DEFAULT_CHAPTER_CUSTOMRENAME),
            &chapter,
            &opts,
        ),
        filename: page_file_name(
            &template(&saveto.filename_rename, DEFAULT_FILENAME_CUSTOMRENAME),
            None,
            0,
        ),
    })
}

/// The padding length FMD2 uses: the configured digits when converting, else none
/// (`OptionConvertDigitVolume`/`OptionConvertDigitChapter`, baseunits/uBaseUnit.pas:1829-1838).
fn digits(convert: bool, length: u32) -> usize {
    if convert {
        usize::try_from(length).unwrap_or(0)
    } else {
        0
    }
}
