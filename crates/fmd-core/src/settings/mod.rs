//! Typed application settings with FMD2's defaults, per-module overrides and limit precedence.

mod model;
mod module_overrides;
mod password;
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
pub use password::verify_password;
pub use service::{FieldError, ModulePatch, SettingsError, SettingsService};
pub use validate::close_rename_tokens;
pub(crate) use validate::{normalize, sync_default_dir};
pub use view::{
    ComicServerSettingsView, ConnectionSettingsView, DiscordSettingsView, HttpOverridesView,
    ModuleUpdaterSettingsView, NotificationSettingsView, NtfySettingsView, ProxyOverrideView,
    ProxySettingsView, ServerSettingsView, SettingsView, WebhookSettingsView,
};
pub use websitebypass::write_websitebypass_config;
