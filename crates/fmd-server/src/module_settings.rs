//! `GET /api/modules` and `GET/PATCH /api/modules/{id}/settings`: a module's declared options
//! with their current values, its limits and the user's overrides
//! (`TWebsiteModuleSettings`, baseunits/WebsiteModulesSettings.pas:56-91).

use axum::Json;
use axum::extract::{Path, State};
use fmd_core::modules::{ModuleInfo, OptionDefKind, SPIN_EDIT_RANGE};
use fmd_core::settings::{HttpOverrides, LimitOverrides, ModuleLimits, ModuleOverrides};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::{ApiError, AppState, Problem};

/// A loaded module, for the module picker.
#[derive(Debug, Serialize, ToSchema)]
pub struct ModuleSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    /// How many options the module declares.
    pub option_count: usize,
}

/// A module's settings.
#[derive(Debug, Serialize, ToSchema)]
pub struct ModuleSettingsView {
    pub id: String,
    pub name: String,
    /// Whether `limits` and `http` apply (`Settings.Enabled`); options always do.
    pub enabled: bool,
    /// The options the module declares with `AddOption*`, in declaration order.
    pub options: Vec<ModuleOptionSetting>,
    /// The user's limit overrides.
    pub limits: LimitOverrides,
    /// The limits the module itself declares; 0 means unlimited.
    pub module_limits: ModuleLimits,
    pub http: HttpOverrides,
}

/// One declared option, its default and the value `MODULE.GetOption` returns for it.
///
/// `key` is the name the value is stored and patched under.
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ModuleOptionSetting {
    /// `AddOptionCheckBox`.
    Checkbox {
        key: String,
        caption: String,
        default: bool,
        value: bool,
    },
    /// `AddOptionEdit`.
    Edit {
        key: String,
        caption: String,
        default: String,
        value: String,
    },
    /// `AddOptionSpinEdit`; FMD2's spin edit spans `min..=max`.
    Spinedit {
        key: String,
        caption: String,
        default: i32,
        value: i32,
        min: i32,
        max: i32,
    },
    /// `AddOptionComboBox`; `default` and `value` index `items`.
    Combobox {
        key: String,
        caption: String,
        items: Vec<String>,
        default: i32,
        value: i32,
    },
}

impl ModuleSettingsView {
    /// Option values resolve like `MODULE.GetOption`: the stored value when it has the option's
    /// type, else the default (`fmd_lua::Module::option_value`,
    /// baseunits/lua/LuaWebsiteModules.pas:921-949).
    fn new(module: ModuleInfo, overrides: ModuleOverrides) -> Self {
        let int = |key: &str| {
            overrides
                .options
                .get(key)
                .and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
        };
        let options = module
            .options
            .into_iter()
            .map(|o| {
                let stored = overrides.options.get(&o.key);
                match o.kind {
                    OptionDefKind::CheckBox { default } => ModuleOptionSetting::Checkbox {
                        value: stored.and_then(Value::as_bool).unwrap_or(default),
                        default,
                        key: o.key,
                        caption: o.caption,
                    },
                    OptionDefKind::Edit { default } => ModuleOptionSetting::Edit {
                        value: stored
                            .and_then(Value::as_str)
                            .map_or_else(|| default.clone(), str::to_owned),
                        default,
                        key: o.key,
                        caption: o.caption,
                    },
                    OptionDefKind::SpinEdit { default } => ModuleOptionSetting::Spinedit {
                        value: int(&o.key).unwrap_or(default),
                        default,
                        min: *SPIN_EDIT_RANGE.start(),
                        max: *SPIN_EDIT_RANGE.end(),
                        key: o.key,
                        caption: o.caption,
                    },
                    OptionDefKind::ComboBox { items, default } => ModuleOptionSetting::Combobox {
                        value: int(&o.key).unwrap_or(default),
                        default,
                        items,
                        key: o.key,
                        caption: o.caption,
                    },
                }
            })
            .collect();
        ModuleSettingsView {
            id: module.id,
            name: module.name,
            enabled: overrides.enabled,
            options,
            limits: overrides.limits,
            module_limits: module.limits,
            http: overrides.http,
        }
    }
}

/// Every loaded module, sorted by ID.
#[utoipa::path(get, path = "/api/modules", tag = "modules", operation_id = "listModules",
    responses((status = 200, body = Vec<ModuleSummary>, description = "The loaded modules")))]
pub(crate) async fn list(State(state): State<AppState>) -> Json<Vec<ModuleSummary>> {
    Json(
        state
            .modules
            .modules()
            .into_iter()
            .map(|m| ModuleSummary {
                option_count: m.options.len(),
                id: m.id,
                name: m.name,
                category: m.category,
            })
            .collect(),
    )
}

/// A module's options, limits and overrides.
#[utoipa::path(get, path = "/api/modules/{id}/settings", tag = "modules",
    operation_id = "getModuleSettings",
    params(("id" = String, Path, description = "Module ID")),
    responses(
        (status = 200, body = ModuleSettingsView, description = "The module's settings"),
        (status = 404, description = "No module with that ID is loaded", body = Problem),
    ))]
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ModuleSettingsView>, ApiError> {
    let module = state.modules.module(&id).ok_or(ApiError::NotFound)?;
    let overrides = state
        .blocking(move |db| ModuleOverrides::load(&db.module_settings(), &id))
        .await?;
    Ok(Json(ModuleSettingsView::new(module, overrides)))
}

/// Update a module's settings with a JSON merge patch (RFC 7396) over `enabled`, `limits`,
/// `http` and `options` (option values keyed by `key`; `null` resets one to its default).
/// Nothing is stored unless the whole patch is valid.
#[utoipa::path(patch, path = "/api/modules/{id}/settings", tag = "modules",
    operation_id = "patchModuleSettings",
    params(("id" = String, Path, description = "Module ID")),
    request_body(content = HashMap<String, Value>, content_type = "application/json"),
    responses(
        (status = 200, body = ModuleSettingsView, description = "The updated settings"),
        (status = 400, description = "The patch is not a JSON object", body = Problem),
        (status = 404, description = "No module with that ID is loaded", body = Problem),
        (status = 422, description = "A value is invalid or a setting unknown; `field` names it",
            body = Problem),
    ))]
pub(crate) async fn patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ApiJson(patch): ApiJson<Value>,
) -> Result<Json<ModuleSettingsView>, ApiError> {
    if !patch.is_object() {
        return Err(ApiError::BadRequest("expected a JSON object".into()));
    }
    let module = state.modules.module(&id).ok_or(ApiError::NotFound)?;
    let options = module.options.clone();
    let overrides = state
        .blocking(move |db| {
            let repo = db.module_settings();
            let mut overrides = ModuleOverrides::load(&repo, &id)?;
            overrides.apply_patch(&options, patch)?;
            overrides.save(&repo, &id)?;
            Ok::<_, ApiError>(overrides)
        })
        .await?;
    Ok(Json(ModuleSettingsView::new(module, overrides)))
}
