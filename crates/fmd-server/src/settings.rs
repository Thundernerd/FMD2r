//! `/api/settings`, `POST /api/preview-rename` and `POST /api/check-folders`.

use axum::Json;
use axum::extract::State;
use std::collections::HashMap;

use fmd_core::download::{SampleChapter, first_page};
use fmd_core::settings::{
    ImageSettings, ModulePatch, OutputSettings, SaveToSettings, Settings, SettingsError,
    SettingsView,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use crate::error::{ApiJson, FieldProblem};
use crate::{ApiError, AppState, ModuleSettingsView, Problem};

impl From<SettingsError> for ApiError {
    fn from(err: SettingsError) -> Self {
        match err {
            SettingsError::Invalid(errors) => ApiError::Fields(
                errors
                    .into_iter()
                    .map(|e| FieldProblem {
                        field: e.field,
                        detail: e.reason,
                    })
                    .collect(),
            ),
            SettingsError::Store(e) => ApiError::Store(e),
            SettingsError::Json(e) => ApiError::Internal(e.to_string()),
            SettingsError::Hash(e) => ApiError::Internal(e),
        }
    }
}

/// All settings.
#[utoipa::path(get, path = "/api/settings", tag = "settings", operation_id = "getSettings",
    responses((status = 200, body = SettingsView, description = "Every setting")))]
pub(crate) async fn get(State(state): State<AppState>) -> Json<SettingsView> {
    Json(state.settings.get().as_ref().into())
}

/// Update settings with a JSON merge patch (RFC 7396) over [`Settings`]; `null` resets a setting
/// to its default. Nothing is stored unless the whole result is valid.
#[utoipa::path(patch, path = "/api/settings", tag = "settings", operation_id = "patchSettings",
    request_body(content = HashMap<String, Value>, content_type = "application/json"),
    responses(
        (status = 200, body = SettingsView, description = "The updated settings"),
        (status = 400, description = "The patch is not a JSON object", body = Problem),
        (status = 422, description = "A value is invalid or a setting unknown; `field` names it",
            body = Problem),
    ))]
pub(crate) async fn patch(
    State(state): State<AppState>,
    ApiJson(patch): ApiJson<Value>,
) -> Result<Json<SettingsView>, ApiError> {
    if !patch.is_object() {
        return Err(ApiError::BadRequest("expected a JSON object".into()));
    }
    let settings = state.settings.clone();
    let before = state.settings.get();
    let updated = crate::state::off_thread(move || settings.update(patch)).await??;
    end_sessions_on_new_password(&state, &before, &updated);
    Ok(Json(updated.as_ref().into()))
}

/// A new stored password ended the login sessions bound to it (see `auth.rs`), so close their
/// event streams.
fn end_sessions_on_new_password(state: &AppState, before: &Settings, after: &Settings) {
    if !state.auth.is_fixed() && before.server.auth_token != after.server.auth_token {
        state.end_sessions();
    }
}

/// What [`patch_all`] saved.
#[derive(Debug, Serialize, ToSchema)]
pub struct SavedSettings {
    pub settings: SettingsView,
    /// The settings of each patched module, by module ID.
    pub modules: HashMap<String, ModuleSettingsView>,
}

/// The body of [`patch_all`]; either part may be left out.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsSave {
    /// A merge patch as for `PATCH /api/settings`.
    #[schema(value_type = Option<HashMap<String, Value>>)]
    pub settings: Option<Map<String, Value>>,
    /// A merge patch as for `PATCH /api/modules/{id}/settings`, by module ID.
    #[schema(value_type = Option<HashMap<String, HashMap<String, Value>>>)]
    pub modules: Option<HashMap<String, Map<String, Value>>>,
}

/// Update the settings and any modules' settings together in one transaction, all or nothing.
#[utoipa::path(patch, path = "/api/settings/all", tag = "settings",
    operation_id = "patchAllSettings",
    request_body(content = SettingsSave, content_type = "application/json"),
    responses(
        (status = 200, body = SavedSettings, description = "The updated settings"),
        (status = 400, description = "Malformed body", body = Problem),
        (status = 422, description = "The body does not have the expected shape", body = Problem),
        (status = 404, description = "No module with a given ID is loaded", body = Problem),
        (status = 422, description = "Values are invalid or settings unknown; `fields` names \
            each, prefixed `settings.` or `modules.<id>.`", body = Problem),
    ))]
pub(crate) async fn patch_all(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<SettingsSave>,
) -> Result<Json<SavedSettings>, ApiError> {
    let patch = Value::Object(body.settings.unwrap_or_default());
    let mut modules = Vec::new();
    let mut infos = Vec::new();
    for (id, patch) in body.modules.unwrap_or_default() {
        let patch = Value::Object(patch);
        let info = state.modules.module(&id).ok_or(ApiError::NotFound)?;
        modules.push(ModulePatch {
            module_id: id,
            options: info.options.clone(),
            patch,
        });
        infos.push(info);
    }
    let settings = state.settings.clone();
    let before = state.settings.get();
    let (updated, overrides) =
        crate::state::off_thread(move || settings.update_with_modules(patch, modules)).await??;
    end_sessions_on_new_password(&state, &before, &updated);
    Ok(Json(SavedSettings {
        settings: updated.as_ref().into(),
        modules: infos
            .into_iter()
            .zip(overrides)
            .map(|(info, o)| (info.id.clone(), ModuleSettingsView::new(info, o)))
            .collect(),
    }))
}

/// The series title leads the chapter name so `remove_manga_name_from_chapter` shows.
const SAMPLE: SampleChapter<'static> = SampleChapter {
    website: "MangaDex",
    title: "Sample Manga",
    authors: "Sample Author",
    artists: "Sample Artist",
    chapter: "Sample Manga - Vol. 1 Ch. 5",
    // FMD2 numbers chapters `%.4d` (mangadownloader/forms/frmMain.pas:2667-2675).
    number: 5,
    page_ext: "jpg",
};

/// The settings a rename preview reads, possibly unsaved; a missing group takes its defaults.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct RenamePreviewRequest {
    pub saveto: SaveToSettings,
    pub images: ImageSettings,
    pub output: OutputSettings,
}

/// The names a download of a sample chapter gets with a draft's settings.
#[derive(Debug, Serialize, ToSchema)]
pub struct RenamePreview {
    /// The manga folder name (`manga_rename`), whether or not the folder is generated.
    pub manga: String,
    /// The chapter folder or archive name (`chapter_rename`).
    pub chapter: String,
    /// The first page's file name, without extension (`filename_rename`).
    pub filename: String,
    /// The first page's file name with the extension it ends up with.
    pub page: String,
    /// The first page's file, or the chapter's archive when chapters are packed.
    pub path: String,
}

/// Preview the naming settings of a (possibly unsaved) draft on a sample chapter: the names
/// and path the download engine gives it.
#[utoipa::path(post, path = "/api/preview-rename", tag = "settings",
    operation_id = "previewRename",
    request_body(content = RenamePreviewRequest, content_type = "application/json"),
    responses(
        (status = 200, body = RenamePreview, description = "The expanded names"),
        (status = 400, description = "Malformed body", body = Problem),
    ))]
pub(crate) async fn preview_rename(
    ApiJson(draft): ApiJson<RenamePreviewRequest>,
) -> Json<RenamePreview> {
    let settings = Settings {
        saveto: draft.saveto,
        images: draft.images,
        output: draft.output,
        ..Settings::default()
    };
    let placement = first_page(&settings, &SAMPLE);
    Json(RenamePreview {
        manga: placement.manga_folder,
        chapter: placement.chapter,
        filename: placement.filename,
        page: placement.page,
        path: placement.path.to_string_lossy().into_owned(),
    })
}

/// Folders to check, such as the destinations of a settings draft.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FolderCheckRequest {
    pub paths: Vec<String>,
}

/// Whether downloads can be saved in one folder.
#[derive(Debug, Serialize, ToSchema)]
pub struct FolderCheck {
    pub path: String,
    /// Why downloads can't be saved there now. A warning, not an error: the first download
    /// creates a missing folder, and a disk may be unmounted for a while.
    pub problem: Option<String>,
}

/// Check whether folders exist and are writable by the server. Relative paths resolve against
/// the server's working directory, as downloads do.
#[utoipa::path(post, path = "/api/check-folders", tag = "settings",
    operation_id = "checkFolders",
    request_body(content = FolderCheckRequest, content_type = "application/json"),
    responses(
        (status = 200, body = Vec<FolderCheck>, description = "One check per path, in order"),
        (status = 400, description = "Malformed body", body = Problem),
    ))]
pub(crate) async fn check_folders(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<FolderCheckRequest>,
) -> Result<Json<Vec<FolderCheck>>, ApiError> {
    let checks = state
        .blocking(move |_| {
            Ok::<_, ApiError>(
                req.paths
                    .into_iter()
                    .map(|path| FolderCheck {
                        problem: folder_problem(std::path::Path::new(path.trim())),
                        path,
                    })
                    .collect(),
            )
        })
        .await?;
    Ok(Json(checks))
}

/// Writability is tried with a scratch file that is removed again.
fn folder_problem(dir: &std::path::Path) -> Option<String> {
    match std::fs::metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Some("the folder does not exist".into());
        }
        Err(e) => return Some(format!("the folder can't be read: {e}")),
        Ok(meta) if !meta.is_dir() => return Some("this is not a folder".into()),
        Ok(_) => {}
    }
    let probe = dir.join(format!(".fmd2r-write-check-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            None
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
        Err(e) => Some(format!("the folder is not writable: {e}")),
    }
}
