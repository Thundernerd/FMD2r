//! What the rest of the app needs to know about a loaded website module: its declared limits and
//! the options its `Init` declared with `AddOption*`, plus the `app.db`-backed store the Lua
//! `MODULE` object reads those options, its cookies and its account from.

use std::ops::RangeInclusive;
use std::sync::Arc;

use fmd_lua::{
    AccountState, ModuleDef, ModuleSettingsStore, OptionKind, OptionValue, SettingsStoreError,
};
use fmd_store::{Account, AppDb, Cipher};
use serde_json::Value;

use crate::settings::ModuleLimits;

/// The range a spin-edit option accepts: `TSpinEditBindValue.Create` sets `MinValue := 0` and
/// `MaxValue := 10000` (mangadownloader/forms/frmWebsiteOptionCustom.pas:162-168).
pub const SPIN_EDIT_RANGE: RangeInclusive<i32> = 0..=10000;

/// A loaded module as the settings see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    /// Lowercased, as the loader leaves it.
    pub root_url: String,
    pub category: String,
    /// The limits the module declares; a negative one counts as 0 (unlimited).
    pub limits: ModuleLimits,
    /// The options a user can set, in declaration order.
    pub options: Vec<OptionDef>,
}

/// One option a module declared with `AddOption*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDef {
    /// The name the value is stored under ([`fmd_lua::ModuleOption::settings_key`]).
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
    /// Keeps the options FMD2 lists in its website options: `TModuleContainer.AddOption` drops
    /// an option without a name (baseunits/WebsiteModules.pas:422-437). One whose cleaned name
    /// is empty is dropped too, as no value could be stored for it.
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
        }
    }
}

/// Splits `text` into lines like assigning `TStrings.Text` (combo items are set that way,
/// mangadownloader/forms/frmWebsiteOptionCustom.pas:410): CR LF, LF and CR all end a line, and a
/// final line break does not add an empty item.
fn text_lines(text: &str) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// A stored option value as the integer a spin edit or combo box holds, if it is one.
pub fn as_i32(value: &Value) -> Option<i32> {
    value.as_i64().and_then(|n| i32::try_from(n).ok())
}

/// The [`ModuleSettingsStore`] over `app.db`'s `module_settings` and `accounts` tables, so
/// option values the settings page saves are what `MODULE.GetOption` returns, and cookies and
/// accounts survive restarts (FMD2's `modules.json`, baseunits/WebsiteModules.pas:545-696).
/// Account credentials and cookies are encrypted with `cipher`.
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

/// Where a manga URL leads: the module handling it and the link FMD2 opens and stores for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located<'a> {
    pub module: &'a ModuleInfo,
    /// The URL's path (and query), relative to the module's `RootURL`.
    pub link: String,
}

/// The module handling `url`, as FMD2's "paste URL" box finds it (`edURLButtonClick`,
/// mangadownloader/forms/frmMain.pas:6578-6606): `SplitURL` splits off the host, which
/// [`locate_by_host`] matches lowercased, and the path is the link. A URL without a host or a
/// path matches nothing. `modules` must be sorted by ID.
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
/// (by ID) whose `RootURL` contains `host`; failing that, the last one containing `host` without
/// its scheme and port; failing that, the last one containing that bare host minus its first
/// four characters, when it starts with `www.` or matches `w+\d*`. FMD2's `Exec` finds that
/// pattern anywhere, so any host holding a `w` gets the second retry. FMD2's shortcut through the
/// last located module is left out: it only changes which of several matching modules wins.
/// `modules` must be sorted by ID.
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
        // `Substring(4)`: everything after the first four characters.
        return pos(&String::from_utf8_lossy(bare.as_bytes().get(4..)?));
    }
    None
}

/// `SplitURL(h, @h, nil, False, False)` (baseunits/httpsendthread.pas:191-276): the host of `url`
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
