//! Per-module overrides (`TWebsiteModuleSettings`, baseunits/WebsiteModulesSettings.pas:56-91)
//! and FMD2's limit precedence.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use fmd_http::{Proxy, ProxyKind};
use fmd_lua::{ModuleHttpOverrides, ModuleHttpSettings, SettingsStoreError};
use fmd_store::{AppDb, ModuleSettings, ModuleSettingsRepo};

use super::model::ConnectionSettings;
use super::service::{FieldError, SettingsError, merge, merge_patch_reporting};
use crate::modules::{OptionDef, OptionDefKind, SPIN_EDIT_RANGE, as_i32};

/// A user's overrides for one module. Everything except `options` only applies while `enabled`
/// is set (`Settings.Enabled`, baseunits/WebsiteModules.pas:362, :400, :408).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ModuleOverrides {
    pub enabled: bool,
    pub limits: LimitOverrides,
    pub http: HttpOverrides,
    /// Values of the options the module declares with `AddOption*`, keyed by option name.
    /// These apply whether or not `enabled` is set.
    #[schema(value_type = Object)]
    pub options: Map<String, Value>,
}

impl ModuleOverrides {
    /// The stored overrides for `module_id`, or the defaults (disabled, nothing overridden) when
    /// none are stored. Missing fields take their defaults; unknown ones are ignored.
    pub fn load(repo: &ModuleSettingsRepo<'_>, module_id: &str) -> Result<Self, SettingsError> {
        let Some(stored) = repo.get(module_id)? else {
            return Ok(Self::default());
        };
        Ok(Self {
            enabled: stored.enabled,
            limits: serde_json::from_value(stored.limits)?,
            http: serde_json::from_value(stored.http)?,
            options: match stored.options {
                Value::Object(map) => map,
                _ => Map::new(),
            },
        })
    }

    /// Stores these overrides for `module_id`, keeping its cookie jar. `http` and `limits` are
    /// merged over what is stored, so keys this build does not know survive. Option values are
    /// replaced wholesale; use `ModuleSettingsRepo::set_option` to change a single option.
    pub fn save(
        &self,
        repo: &ModuleSettingsRepo<'_>,
        module_id: &str,
    ) -> Result<(), SettingsError> {
        repo.upsert(&self.merged_over_stored(repo, module_id)?)?;
        Ok(())
    }

    /// What [`Self::save`] stores: these overrides merged over the stored ones.
    pub(super) fn merged_over_stored(
        &self,
        repo: &ModuleSettingsRepo<'_>,
        module_id: &str,
    ) -> Result<ModuleSettings, SettingsError> {
        let mut stored = repo
            .get(module_id)?
            .unwrap_or_else(|| ModuleSettings::new(module_id));
        stored.enabled = self.enabled;
        merge(&mut stored.limits, serde_json::to_value(self.limits)?);
        merge(&mut stored.http, serde_json::to_value(&self.http)?);
        stored.options = Value::Object(self.options.clone());
        Ok(stored)
    }
}

impl ModuleOverrides {
    /// Applies `patch`, a JSON merge patch over these overrides, checking option values against
    /// the module's declared `options`: an option must be declared, a checkbox takes a boolean,
    /// an edit a string, a spin edit an integer in [`SPIN_EDIT_RANGE`] and a combo box the index
    /// of one of its items (`csDropDownList`,
    /// mangadownloader/forms/frmWebsiteOptionCustom.pas:187-191). `null` resets an option to
    /// its default.
    ///
    /// On error `self` is unchanged; the error lists every offending field as a dotted path.
    pub fn apply_patch(
        &mut self,
        options: &[OptionDef],
        patch: Value,
    ) -> Result<(), SettingsError> {
        *self = self.patched(options, patch)?;
        Ok(())
    }

    /// These overrides with `patch` applied, as [`Self::apply_patch`] checks it.
    pub fn patched(&self, options: &[OptionDef], patch: Value) -> Result<Self, SettingsError> {
        let Value::Object(mut patch) = patch else {
            return Err(SettingsError::Invalid(vec![FieldError::new(
                "",
                "expected a JSON object",
            )]));
        };
        let mut errors = Vec::new();
        let option_patch = patch.remove("options");
        let mut base = serde_json::to_value(self)?;
        if let Some(map) = base.as_object_mut() {
            map.remove("options");
        }
        let next: Option<ModuleOverrides> =
            merge_patch_reporting(base, Value::Object(patch), &mut errors);
        let mut next_options = self.options.clone();
        match option_patch {
            None | Some(Value::Null) => {}
            Some(Value::Object(values)) => {
                for (key, value) in values {
                    let field = format!("options.{key}");
                    let Some(def) = options.iter().find(|o| o.key == key) else {
                        errors.push(FieldError::unknown(field));
                        continue;
                    };
                    if value.is_null() {
                        next_options.remove(&key);
                    } else if let Err(reason) = check_option(def, &value) {
                        errors.push(FieldError::new(field, reason));
                    } else {
                        next_options.insert(key, value);
                    }
                }
            }
            Some(_) => errors.push(FieldError::new("options", "expected a JSON object")),
        }
        // `next` is only missing after an error was reported.
        let Some(mut next) = next.filter(|_| errors.is_empty()) else {
            return Err(SettingsError::Invalid(errors));
        };
        next.options = next_options;
        Ok(next)
    }
}

/// Why `value` is not a valid value of option `def`, if it is not.
fn check_option(def: &OptionDef, value: &Value) -> Result<(), String> {
    match &def.kind {
        OptionDefKind::CheckBox { .. } if value.is_boolean() => Ok(()),
        OptionDefKind::CheckBox { .. } => Err("expected true or false".into()),
        OptionDefKind::Edit { .. } if value.is_string() => Ok(()),
        OptionDefKind::Edit { .. } => Err("expected a string".into()),
        OptionDefKind::SpinEdit { .. } => match as_i32(value) {
            Some(n) if SPIN_EDIT_RANGE.contains(&n) => Ok(()),
            _ => Err(format!(
                "expected an integer in {}..={}",
                SPIN_EDIT_RANGE.start(),
                SPIN_EDIT_RANGE.end()
            )),
        },
        OptionDefKind::ComboBox { items, .. } => match as_i32(value) {
            Some(n) if usize::try_from(n).is_ok_and(|i| i < items.len()) => Ok(()),
            _ => Err(format!(
                "expected the index of an item, 0..={}",
                items.len().saturating_sub(1)
            )),
        },
    }
}

/// One module's HTTP settings in `app.db`, as its `HTTP` objects read them before every request
/// and the anti-bot hook writes them (`TModuleContainer.Settings.HTTP`,
/// baseunits/WebsiteModules.pas:278-283, :353-379; baseunits/lua/LuaWebsiteBypass.pas:178-184).
pub struct StoredModuleHttpSettings {
    db: AppDb,
    module_id: String,
}

impl StoredModuleHttpSettings {
    pub fn new(db: AppDb, module_id: impl Into<String>) -> Self {
        Self {
            db,
            module_id: module_id.into(),
        }
    }

    /// Loads the module's overrides, changes them with `change` and stores them.
    fn update(&self, change: impl FnOnce(&mut ModuleOverrides)) -> Result<(), SettingsStoreError> {
        let repo = self.db.module_settings();
        let mut overrides =
            ModuleOverrides::load(&repo, &self.module_id).map_err(SettingsStoreError::new)?;
        change(&mut overrides);
        overrides
            .save(&repo, &self.module_id)
            .map_err(SettingsStoreError::new)
    }
}

impl ModuleHttpSettings for StoredModuleHttpSettings {
    fn http_overrides(&self) -> Option<ModuleHttpOverrides> {
        let overrides = match ModuleOverrides::load(&self.db.module_settings(), &self.module_id) {
            Ok(overrides) => overrides,
            Err(e) => {
                tracing::error!(target: "fmd_core", "settings of module {}: {e}", self.module_id);
                return None;
            }
        };
        let http = overrides.http;
        overrides.enabled.then(|| ModuleHttpOverrides {
            user_agent: http.user_agent,
            cookies: http.cookies,
            proxy: http.proxy.to_lua(),
        })
    }

    fn clear_cookies(&self) -> Result<(), SettingsStoreError> {
        self.update(|overrides| overrides.http.cookies.clear())
    }

    fn store_bypass(&self, cookies: &str, user_agent: &str) -> Result<(), SettingsStoreError> {
        self.update(|overrides| {
            overrides.enabled = true;
            overrides.http.cookies = cookies.to_owned();
            overrides.http.user_agent = user_agent.to_owned();
        })
    }
}

/// Limit overrides; 0 means "not overridden" for tasks and threads
/// (baseunits/WebsiteModulesSettings.pas:81-83).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct LimitOverrides {
    pub max_task_limit: u32,
    pub max_thread_per_task_limit: u32,
    /// Replaces the module's connection limit while enabled, 0 (unlimited) included
    /// (baseunits/WebsiteModulesSettings.pas:126-155).
    pub max_connection_limit: u32,
}

/// HTTP overrides applied in `PrepareHTTP` (baseunits/WebsiteModules.pas:353-379).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct HttpOverrides {
    /// Replaces the default user agent when non-empty.
    pub user_agent: String,
    /// Cookies merged into every request (`HTTP.Cookies`,
    /// baseunits/WebsiteModulesSettings.pas:41).
    pub cookies: String,
    pub proxy: ProxyOverride,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ProxyOverride {
    #[serde(rename = "type")]
    pub kind: ProxyOverrideType,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
}

impl ProxyOverride {
    /// The override as `fmd-lua` applies it to a session (baseunits/WebsiteModules.pas:364-379).
    fn to_lua(&self) -> fmd_lua::ProxyOverride {
        let kind = match self.kind {
            ProxyOverrideType::Default => return fmd_lua::ProxyOverride::Default,
            ProxyOverrideType::Direct => return fmd_lua::ProxyOverride::Direct,
            ProxyOverrideType::Http => ProxyKind::Http,
            ProxyOverrideType::Socks4 => ProxyKind::Socks4,
            ProxyOverrideType::Socks5 => ProxyKind::Socks5,
        };
        fmd_lua::ProxyOverride::Proxy(Proxy {
            kind,
            host: self.host.clone(),
            port: self.port.clone(),
            user: self.username.clone(),
            pass: self.password.clone(),
        })
    }
}

/// `TProxyType` (baseunits/WebsiteModulesSettings.pas:11).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProxyOverrideType {
    /// Use the global proxy settings.
    #[default]
    Default,
    /// No proxy for this module.
    Direct,
    Http,
    Socks4,
    Socks5,
}

/// The limits a module declares (`MaxTaskLimit`, `MaxThreadPerTaskLimit`, `MaxConnectionLimit`
/// on the Lua `MODULE` object); 0 means unlimited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct ModuleLimits {
    pub max_task_limit: u32,
    pub max_thread_per_task_limit: u32,
    pub max_connection_limit: u32,
}

/// The limits that apply to one module's downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveLimits {
    /// Tasks of this module that may run at once. The global `max_parallel_tasks` still bounds
    /// all modules' tasks together.
    pub max_tasks: u32,
    /// Page threads per task; at least 1.
    pub threads_per_task: u32,
    /// Concurrent connections to the module's host; 0 means unlimited.
    pub max_connections: u32,
}

/// Combines a module's declared limits, the user's overrides and the global connection settings
/// with FMD2's precedence.
///
/// - Tasks: `GetMaxTaskLimit` (baseunits/WebsiteModules.pas:398-404) picks a non-zero enabled
///   override over the module's limit; `CanCreateTask` (:414-420) treats 0 as unlimited; the
///   download manager also caps the total at `OptionMaxParallel`
///   (baseunits/uDownloadsManager.pas:1798-1803).
/// - Connections: an enabled override replaces the module's limit
///   (baseunits/WebsiteModulesSettings.pas:126-155).
/// - Threads: `TTaskThread.GetCurrentLimit` (baseunits/uDownloadsManager.pas:868-880) takes the
///   module's limit, or the global one when it is 0, caps it at the global limit and the
///   connection limit, and floors it at 1. FMD2 reads the module's raw `MaxThreadPerTaskLimit`
///   there, ignoring the override; FMD2r uses the override-aware `GetMaxThreadPerTaskLimit`
///   (baseunits/WebsiteModules.pas:406-412) so the per-module setting takes effect.
pub fn effective_limits(
    module: &ModuleLimits,
    overrides: Option<&ModuleOverrides>,
    global: &ConnectionSettings,
) -> EffectiveLimits {
    let enabled = overrides.filter(|o| o.enabled).map(|o| o.limits);
    let pick = |overridden: Option<u32>, declared: u32| match overridden {
        Some(value) if value != 0 => value,
        _ => declared,
    };

    // A declared or overridden limit of 0 means "use the global limit"; any other limit is
    // still capped by it.
    let capped = |limit: u32, global: u32| match limit {
        0 => global,
        limit => limit.min(global),
    };
    let max_tasks = capped(
        pick(enabled.map(|l| l.max_task_limit), module.max_task_limit),
        global.max_parallel_tasks,
    );

    let max_connections = enabled.map_or(module.max_connection_limit, |l| l.max_connection_limit);

    let mut threads_per_task = capped(
        pick(
            enabled.map(|l| l.max_thread_per_task_limit),
            module.max_thread_per_task_limit,
        ),
        global.threads_per_task,
    );
    if max_connections > 0 {
        threads_per_task = threads_per_task.min(max_connections);
    }

    EffectiveLimits {
        max_tasks,
        threads_per_task: threads_per_task.max(1),
        max_connections,
    }
}
