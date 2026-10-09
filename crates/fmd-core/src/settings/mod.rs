//! The settings model: typed application settings with FMD2's defaults, persisted in `app.db`,
//! plus per-module overrides and FMD2's limit precedence.

mod model;
mod module_overrides;
mod secrets;
mod service;
mod validate;
mod view;
mod websitebypass;

pub use model::*;
pub use module_overrides::{
    EffectiveLimits, HttpOverrides, LimitOverrides, ModuleLimits, ModuleOverrides, ProxyOverride,
    ProxyOverrideType, StoredModuleHttpSettings, effective_limits,
};
pub use service::{FieldError, ModulePatch, SettingsError, SettingsService};
pub(crate) use validate::normalize;
pub use view::{
    ConnectionSettingsView, HttpOverridesView, ModuleUpdaterSettingsView, ProxyOverrideView,
    ProxySettingsView, ServerSettingsView, SettingsView,
};
pub use websitebypass::write_websitebypass_config;
