//! Loaded modules' declared limits and options, and the `app.db`-backed module settings store.

use std::ops::RangeInclusive;
use std::sync::Arc;

use fmd_lua::{
    AccountState, ModuleDef, ModuleSettingsStore, OptionKind, OptionValue, SettingsStoreError,
};
use fmd_store::{Account, AppDb, Cipher};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::settings::ModuleLimits;

/// `TSpinEditBindValue.Create`'s `MinValue`/`MaxValue`
/// (mangadownloader/forms/frmWebsiteOptionCustom.pas:162-168).
pub const SPIN_EDIT_RANGE: RangeInclusive<i32> = 0..=10000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    /// Lowercased.
    pub root_url: String,
    pub category: String,
    /// A negative limit counts as 0 (unlimited).
    pub limits: ModuleLimits,
    /// In declaration order.
    pub options: Vec<OptionDef>,
    pub capabilities: ModuleCapabilities,
}

/// What a module can do, from the callbacks and flags it declares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct ModuleCapabilities {
    /// Its list can be updated: it declares `OnGetNameAndLink`.
    pub update_list: bool,
    /// It reads manga info: it declares `OnGetInfo` (`InformationAvailable` aside).
    pub info: bool,
    /// It downloads chapters: it declares `OnGetPageNumber`.
    pub download: bool,
    /// It has an account to log in with (`AccountSupport`).
    pub account: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDef {
    /// [`fmd_lua::ModuleOption::settings_key`].
    pub key: String,
    pub caption: String,
    pub kind: OptionDefKind,
}

/// An option's kind and default (`TWebsiteOptionType`, baseunits/WebsiteModules.pas:68).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionDefKind {
    CheckBox {
        default: bool,
    },
    Edit {
        default: String,
    },
    SpinEdit {
        default: i32,
    },
    /// `default` is the index of the selected item.
    ComboBox {
        items: Vec<String>,
        default: i32,
    },
}

impl From<&ModuleDef> for ModuleInfo {
    /// Drops unnamed options like `TModuleContainer.AddOption`
    /// (baseunits/WebsiteModules.pas:422-437), and ones whose cleaned name is empty.
    fn from(def: &ModuleDef) -> Self {
        let limit = |value: i32| u32::try_from(value).unwrap_or(0);
        ModuleInfo {
            id: def.id.clone(),
            name: def.name.clone(),
            root_url: def.root_url.clone(),
            category: def.category.clone(),
            limits: ModuleLimits {
                max_task_limit: limit(def.max_task_limit),
                max_thread_per_task_limit: limit(def.max_thread_per_task_limit),
                max_connection_limit: limit(def.max_connection_limit),
            },
            options: def
                .options
                .iter()
                .filter(|o| !o.name.is_empty() && !o.settings_key().is_empty())
                .map(|o| OptionDef {
                    key: o.settings_key(),
                    caption: o.caption.clone(),
                    kind: match &o.kind {
                        OptionKind::CheckBox { default } => {
                            OptionDefKind::CheckBox { default: *default }
                        }
                        OptionKind::Edit { default } => OptionDefKind::Edit {
                            default: default.clone(),
                        },
                        OptionKind::SpinEdit { default } => {
                            OptionDefKind::SpinEdit { default: *default }
                        }
                        OptionKind::ComboBox { items, default } => OptionDefKind::ComboBox {
                            items: text_lines(items),
                            default: *default,
                        },
                    },
                })
                .collect(),
            capabilities: ModuleCapabilities {
                update_list: def.on_get_name_and_link.is_some(),
                info: def.on_get_info.is_some(),
                download: def.on_get_page_number.is_some(),
                account: def.account_support,
            },
        }
    }
}

/// Like assigning `TStrings.Text` (mangadownloader/forms/frmWebsiteOptionCustom.pas:410): CR LF,
/// LF and CR end a line; a final line break adds no empty item.
fn text_lines(text: &str) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// A stored spin edit or combo box value.
pub fn as_i32(value: &Value) -> Option<i32> {
    value.as_i64().and_then(|n| i32::try_from(n).ok())
}

/// The [`ModuleSettingsStore`] over `app.db` (FMD2's `modules.json`,
/// baseunits/WebsiteModules.pas:545-696). Account credentials and cookies are encrypted.
pub struct StoreModuleSettings {
    db: AppDb,
    cipher: Arc<dyn Cipher>,
}

impl StoreModuleSettings {
    pub fn new(db: AppDb, cipher: Arc<dyn Cipher>) -> Self {
        Self { db, cipher }
    }
}

impl ModuleSettingsStore for StoreModuleSettings {
    fn option(
        &self,
        module_id: &str,
        name: &str,
    ) -> Result<Option<OptionValue>, SettingsStoreError> {
        let stored = self
            .db
            .module_settings()
            .option(module_id, name)
            .map_err(SettingsStoreError::new)?;
        Ok(stored.and_then(|value| match value {
            Value::Bool(b) => Some(OptionValue::Bool(b)),
            Value::String(s) => Some(OptionValue::Text(s)),
            other => as_i32(&other).map(OptionValue::Integer),
        }))
    }

    fn set_option(
        &self,
        module_id: &str,
        name: &str,
        value: OptionValue,
    ) -> Result<(), SettingsStoreError> {
        let value = match value {
            OptionValue::Bool(b) => Value::Bool(b),
            OptionValue::Text(s) => Value::String(s),
            OptionValue::Integer(n) => Value::from(n),
        };
        self.db
            .module_settings()
            .set_option(module_id, name, &value)
            .map_err(SettingsStoreError::new)
    }

    fn cookies(&self, module_id: &str) -> Result<Option<String>, SettingsStoreError> {
        let jar = self
            .db
            .module_settings()
            .cookie_jar(module_id)
            .map_err(SettingsStoreError::new)?;
        jar.map(|bytes| String::from_utf8(bytes).map_err(SettingsStoreError::new))
            .transpose()
    }

    fn set_cookies(&self, module_id: &str, cookies: &str) -> Result<(), SettingsStoreError> {
        self.db
            .module_settings()
            .set_cookie_jar(module_id, Some(cookies.as_bytes()))
            .map_err(SettingsStoreError::new)
    }

    fn account(&self, module_id: &str) -> Result<Option<AccountState>, SettingsStoreError> {
        let stored = self
            .db
            .accounts(self.cipher.as_ref())
            .get(module_id)
            .map_err(SettingsStoreError::new)?;
        Ok(stored.map(|a| AccountState {
            enabled: a.enabled,
            username: a.username,
            password: a.password,
            status: crate::accounts::status_ordinal(a.status),
            cookies: a.cookies,
        }))
    }

    fn set_account(
        &self,
        module_id: &str,
        account: &AccountState,
    ) -> Result<(), SettingsStoreError> {
        self.db
            .accounts(self.cipher.as_ref())
            .upsert(&Account {
                module_id: module_id.to_owned(),
                enabled: account.enabled,
                username: account.username.clone(),
                password: account.password.clone(),
                cookies: account.cookies.clone(),
                status: crate::accounts::status_of(account.status),
            })
            .map_err(SettingsStoreError::new)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located<'a> {
    pub module: &'a ModuleInfo,
    /// Path and query, relative to the module's `RootURL`.
    pub link: String,
}

/// FMD2's "paste URL" lookup (`edURLButtonClick`, mangadownloader/forms/frmMain.pas:6578-6606).
/// A URL without a host or a path matches nothing. `modules` must be sorted by ID.
pub fn locate_by_url<'a>(modules: &'a [ModuleInfo], url: &str) -> Option<Located<'a>> {
    let (host, link) = fmd_http::split_url_bytes(url.as_bytes());
    if host.is_empty() || link.is_empty() {
        return None;
    }
    let host = String::from_utf8_lossy(&host).to_lowercase();
    let module = locate_by_host(modules, &host)?;
    Some(Located {
        module,
        link: String::from_utf8_lossy(&link).into_owned(),
    })
}

/// `TWebsiteModules.LocateModuleByHost` (baseunits/WebsiteModules.pas:500-530): the last module
/// whose `RootURL` contains `host`, else the bare host, else the bare host minus its first four
/// characters when it matches `w+\d*` (FMD2's `Exec` matches anywhere, so any `w` will do).
/// FMD2's last-located shortcut is left out. `modules` must be sorted by ID.
pub fn locate_by_host<'a>(modules: &'a [ModuleInfo], host: &str) -> Option<&'a ModuleInfo> {
    // `Pos` of an empty string is 0, so it matches nothing.
    let pos = |s: &str| {
        modules
            .iter()
            .rev()
            .find(|m| !s.is_empty() && m.root_url.contains(s))
    };
    let host = host.to_lowercase();
    if let Some(module) = pos(&host) {
        return Some(module);
    }
    let bare = bare_host(&host);
    if bare.is_empty() {
        return None;
    }
    if let Some(module) = pos(&bare) {
        return Some(module);
    }
    if bare.starts_with("www.") || bare.contains('w') {
        // `Substring(4)`.
        return pos(&String::from_utf8_lossy(bare.as_bytes().get(4..)?));
    }
    None
}

/// `SplitURL(h, @h, nil, False, False)` (baseunits/httpsendthread.pas:191-276): the host
/// without scheme or port.
fn bare_host(url: &str) -> String {
    let (host, _) = fmd_http::split_url_bytes(url.as_bytes());
    let host = String::from_utf8_lossy(&host);
    // `split_url_bytes` always adds a scheme, and the port it found as `:<digits>`.
    let host = host.split_once("://").map_or(&*host, |(_, rest)| rest);
    match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            name.to_owned()
        }
        _ => host.to_owned(),
    }
}
