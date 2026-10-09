//! `GET /api/modules` and `GET/PATCH /api/modules/{id}/settings` (`TWebsiteModuleSettings`,
//! baseunits/WebsiteModulesSettings.pas:56-91).

use axum::Json;
use axum::extract::{Path, State};
use std::collections::HashMap;

use fmd_core::modules::{ModuleCapabilities, ModuleInfo, OptionDefKind, SPIN_EDIT_RANGE, as_i32};
use fmd_core::settings::{
    HttpOverrides, HttpOverridesView, LimitOverrides, ModuleLimits, ModuleOverrides,
};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use fmd_store::{AppDb, ListSummary};

use crate::error::ApiJson;
use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

/// A loaded module, for the module pickers.
///
/// IDs can repeat: FMD2's loader does not check them (baseunits/lua/LuaWebsiteModules.pas:523-589),
/// and `lua/modules/Manga1001.lua:18-19` registers two websites under one ID. `root_url` tells
/// them apart; they share the ID's settings (baseunits/WebsiteModules.pas:545-700).
#[derive(Debug, Serialize, ToSchema)]
pub struct ModuleSummary {
    pub id: String,
    pub name: String,
    /// The website's root URL, lowercased as the loader leaves it.
    pub root_url: String,
    pub category: String,
    /// How many options the module declares.
    pub option_count: usize,
    pub capabilities: ModuleCapabilities,
    /// Titles in its list (`lists.db`).
    pub list_size: u64,
    /// RFC 3339 time its list last changed through an update or import.
    pub list_updated: Option<String>,
    /// Whether a list update or import of it is running.
    pub list_job_running: bool,
    /// Whether its settings differ from the defaults: a changed option, enabled overrides
    /// (`Settings.Enabled`, baseunits/WebsiteModulesSettings.pas:80) that change a limit or HTTP
    /// setting, or its own download folder.
    pub customized: bool,
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
    pub http: HttpOverridesView,
    /// The website's download folder when the user picks none; empty for the default
    /// (`OverrideSettings.SaveToPath`, baseunits/WebsiteModulesSettings.pas:50). Applies even
    /// when not `enabled`.
    pub save_to: String,
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
    fn options_changed(&self) -> bool {
        self.options.iter().any(|o| match o {
            // The arms differ in their fields' types, so they can't share a pattern.
            ModuleOptionSetting::Checkbox { default, value, .. } => default != value,
            ModuleOptionSetting::Edit { default, value, .. } => default != value,
            ModuleOptionSetting::Spinedit { default, value, .. }
            | ModuleOptionSetting::Combobox { default, value, .. } => default != value,
        })
    }

    /// Option values resolve like `MODULE.GetOption`: the stored value when it has the option's
    /// type, else the default (`fmd_lua::Module::option_value`,
    /// baseunits/lua/LuaWebsiteModules.pas:921-949).
    pub(crate) fn new(module: ModuleInfo, overrides: ModuleOverrides) -> Self {
        let int = |key: &str| overrides.options.get(key).and_then(as_i32);
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
            http: overrides.http.into(),
            save_to: overrides.save_to,
        }
    }
}

/// Every loaded module, sorted by ID; modules sharing an ID are all listed.
#[utoipa::path(get, path = "/api/modules", tag = "modules", operation_id = "listModules",
    responses((status = 200, body = Vec<ModuleSummary>, description = "The loaded modules")))]
pub(crate) async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<ModuleSummary>>, ApiError> {
    let lists: HashMap<String, ListSummary> = match state.lists.clone() {
        Some(lists) => off_thread(move || lists.masterlist().summaries())
            .await??
            .into_iter()
            .map(|s| (s.module_id.clone(), s))
            .collect(),
        None => HashMap::new(),
    };
    let overrides = state.blocking(stored_overrides).await?;
    Ok(Json(
        state
            .modules
            .modules()
            .into_iter()
            .map(|m| {
                let list = lists.get(&m.id);
                let customized = overrides
                    .get(&m.id)
                    .is_some_and(|o| customized(m.clone(), o.clone()));
                ModuleSummary {
                    customized,
                    option_count: m.options.len(),
                    capabilities: m.capabilities,
                    list_size: list.map_or(0, |l| l.count),
                    list_updated: list
                        .and_then(|l| l.updated_at)
                        .map(crate::time::rfc3339_from_unix_ms),
                    list_job_running: state
                        .list_jobs
                        .as_ref()
                        .is_some_and(|jobs| jobs.is_running(&m.id)),
                    id: m.id,
                    name: m.name,
                    root_url: m.root_url,
                    category: m.category,
                }
            })
            .collect(),
    ))
}

/// See [`ModuleSummary::customized`]. Enabled overrides replace the module's connection limit even
/// at 0 (unlimited) (baseunits/WebsiteModulesSettings.pas:126-155), lifting a declared one.
fn customized(module: ModuleInfo, overrides: ModuleOverrides) -> bool {
    let overridden = overrides.enabled
        && (overrides.limits != LimitOverrides::default()
            || overrides.http != HttpOverrides::default()
            || module.limits.max_connection_limit != 0);
    overridden
        || !overrides.save_to.is_empty()
        || ModuleSettingsView::new(module, overrides).options_changed()
}

/// A module whose overrides fail to load is left out, so one bad row does not hide the list.
fn stored_overrides(db: &AppDb) -> Result<HashMap<String, ModuleOverrides>, ApiError> {
    let repo = db.module_settings();
    let ids = repo
        .module_ids()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(ids
        .into_iter()
        .filter_map(|id| match ModuleOverrides::load(&repo, &id) {
            Ok(overrides) => Some((id, overrides)),
            Err(e) => {
                tracing::warn!(target: "fmd_server", "settings of module {id}: {e}");
                None
            }
        })
        .collect())
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
/// `http`, `save_to` and `options` (option values keyed by `key`; `null` resets one to its default).
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
