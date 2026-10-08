//! The settings model: typed application settings with FMD2's defaults, persisted in `app.db`,
//! plus per-module overrides and FMD2's limit precedence.

mod model;
mod module_overrides;
mod service;
mod validate;

pub use model::*;
pub use module_overrides::{
    EffectiveLimits, HttpOverrides, LimitOverrides, ModuleLimits, ModuleOverrides, ProxyOverride,
    ProxyOverrideType, effective_limits,
};
pub use service::{SettingsError, SettingsService};
