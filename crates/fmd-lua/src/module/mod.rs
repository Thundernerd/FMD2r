//! Website modules: what a module's `Init()` declares ([`ModuleDef`]), the state its `MODULE`
//! object shares across threads ([`Module`]), and the registry the loader fills
//! (baseunits/lua/LuaWebsiteModules.pas, baseunits/WebsiteModules.pas).

mod loader;
mod object;
mod settings;
mod sync;

use std::path::PathBuf;
use std::sync::atomic::AtomicI32;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};

use fmd_http::ModuleHttp;

use crate::StringList;

pub use loader::{LoadFailure, LoadReport, ModuleRegistry};
pub(crate) use object::build_module;
pub use settings::{MemorySettingsStore, ModuleSettingsStore, OptionValue, SettingsStoreError};
pub use sync::CriticalSection;

/// Everything a module's `Init` set on its `MODULE` object
/// (`TModuleContainer` fields, baseunits/WebsiteModules.pas:105-182, and the callback names of
/// `TLuaWebsiteModule`, baseunits/lua/LuaWebsiteModules.pas:15-54).
///
/// A callback is `None` when its name is empty, as FMD2 only wires non-empty names
/// (baseunits/lua/LuaWebsiteModules.pas:554-584).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDef {
    pub id: String,
    pub name: String,
    /// Lowercased once `Init` has run (baseunits/lua/LuaWebsiteModules.pas:553).
    pub root_url: String,
    pub category: String,
    pub max_task_limit: i32,
    pub max_thread_per_task_limit: i32,
    pub max_connection_limit: i32,
    pub sorted_list: bool,
    pub information_available: bool,
    pub favorite_available: bool,
    pub dynamic_page_link: bool,
    pub total_directory: i32,
    pub account_support: bool,
    pub tag: i32,
    pub last_updated: String,
    pub current_directory_index: i32,
    pub on_before_update_list: Option<String>,
    pub on_after_update_list: Option<String>,
    pub on_get_directory_page_number: Option<String>,
    pub on_get_name_and_link: Option<String>,
    pub on_get_info: Option<String>,
    pub on_task_start: Option<String>,
    pub on_get_page_number: Option<String>,
    pub on_get_image_url: Option<String>,
    pub on_before_download_image: Option<String>,
    pub on_download_image: Option<String>,
    pub on_save_image: Option<String>,
    pub on_after_image_saved: Option<String>,
    pub on_login: Option<String>,
    pub on_account_state: Option<String>,
    pub on_check_site: Option<String>,
    /// The options declared with `AddOption*`, in declaration order.
    pub options: Vec<ModuleOption>,
    /// The module file whose `Init` declared the module.
    pub file: PathBuf,
}

impl ModuleDef {
    /// A module before `Init` sets anything, with `TModuleContainer.Create`'s defaults
    /// (baseunits/WebsiteModules.pas:296-312).
    fn new(file: PathBuf) -> Self {
        ModuleDef {
            id: String::new(),
            name: String::new(),
            root_url: String::new(),
            category: String::new(),
            max_task_limit: 0,
            max_thread_per_task_limit: 0,
            max_connection_limit: 0,
            sorted_list: false,
            information_available: true,
            favorite_available: true,
            dynamic_page_link: false,
            total_directory: 1,
            account_support: false,
            tag: 0,
            last_updated: String::new(),
            current_directory_index: 0,
            on_before_update_list: None,
            on_after_update_list: None,
            on_get_directory_page_number: None,
            on_get_name_and_link: None,
            on_get_info: None,
            on_task_start: None,
            on_get_page_number: None,
            on_get_image_url: None,
            on_before_download_image: None,
            on_download_image: None,
            on_save_image: None,
            on_after_image_saved: None,
            on_login: None,
            on_account_state: None,
            on_check_site: None,
            options: Vec::new(),
            file,
        }
    }

    /// The option named `name`, ignoring ASCII case like the sorted `TStringList` FMD2 keeps
    /// them in (baseunits/lua/LuaWebsiteModules.pas:757-766, :928).
    pub fn option(&self, name: &str) -> Option<&ModuleOption> {
        self.options
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name))
    }
}

/// One option a module declared with `AddOption*` (baseunits/lua/LuaWebsiteModules.pas:768-818).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleOption {
    pub name: String,
    pub caption: String,
    pub kind: OptionKind,
}

impl ModuleOption {
    /// The name the option's value is saved under: `CleanOptionName` of its name, trimmed, without
    /// leading digits and keeping only ASCII letters, digits and `_`
    /// (baseunits/WebsiteModules.pas:223-241, applied at :433). Empty for an option the settings
    /// never hold.
    pub fn settings_key(&self) -> String {
        self.name
            .trim()
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    }
}

/// The kind of an option, with its default value (`TOptionItem*`,
/// baseunits/lua/LuaWebsiteModules.pas:113-132).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionKind {
    CheckBox {
        default: bool,
    },
    Edit {
        default: String,
    },
    SpinEdit {
        default: i32,
    },
    /// `items` holds the choices one per line; `default` is the selected index.
    ComboBox {
        items: String,
        default: i32,
    },
}

/// A module's account (`TWebsiteModuleAccount`, baseunits/WebsiteModules.pas:82-98), present
/// while `AccountSupport` is true.
#[derive(Default)]
pub struct Account {
    state: Mutex<AccountState>,
    guardian: Arc<CriticalSection>,
}

/// The fields of an [`Account`]. Its `Debug` output leaves the credentials and cookies out, so
/// they never reach a log.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AccountState {
    pub enabled: bool,
    pub username: String,
    pub password: String,
    /// `TAccountStatus` as its ordinal: 0 unknown, 1 checking, 2 valid, 3 invalid.
    pub status: i32,
    pub cookies: String,
}

impl AccountState {
    /// `asChecking`, the ordinal of `TAccountStatus.asChecking` (baseunits/WebsiteModules.pas:78).
    pub const CHECKING: i32 = 1;
    /// `asUnknown` (baseunits/WebsiteModules.pas:78).
    pub const UNKNOWN: i32 = 0;
}

impl std::fmt::Debug for AccountState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountState")
            .field("enabled", &self.enabled)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Account {
    /// A copy of the account's fields.
    pub fn state(&self) -> AccountState {
        lock(&self.state).clone()
    }

    /// Replaces the account's fields.
    pub fn set_state(&self, state: AccountState) {
        *lock(&self.state) = state;
    }

    /// The account's `Guardian` critical section.
    pub fn guardian(&self) -> &Arc<CriticalSection> {
        &self.guardian
    }
}

/// The limits of a module's downloads, 0 meaning none: `MaxTaskLimit`, `MaxThreadPerTaskLimit`
/// and `MaxConnectionLimit` (baseunits/WebsiteModules.pas:136-137,
/// baseunits/lua/LuaWebsiteModules.pas:1001-1003).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModuleLimits {
    pub max_task_limit: i32,
    pub max_thread_per_task_limit: i32,
    pub max_connection_limit: i32,
}

/// The `Storage` of a module (`TStringsStorage`, baseunits/lua/LuaStringsStorage.pas:13-31).
struct Storage {
    values: StringList,
    tag: i32,
    enable: bool,
    status: Vec<u8>,
}

impl Default for Storage {
    /// `TStringsStorage.Create` (baseunits/lua/LuaStringsStorage.pas:72-80).
    fn default() -> Self {
        Storage {
            values: StringList::new(),
            tag: 0,
            enable: true,
            status: b"my status of TStringsStorage".to_vec(),
        }
    }
}

/// One website module: its [`ModuleDef`] and the state every Lua state running it shares, the
/// way all of FMD2's threads share one `TModuleContainer` and `TLuaWebsiteModule`. Every
/// `MODULE` object built over the same `Module` reads and writes the same properties, options,
/// cookies, `Storage`, `Guardian` and `Account`.
pub struct Module {
    def: RwLock<ModuleDef>,
    storage: Mutex<Storage>,
    guardian: Arc<CriticalSection>,
    /// `TWebsiteBypass.Guardian`: serialises the module's anti-bot bypasses
    /// (baseunits/lua/LuaWebsiteBypass.pas:14-20, :161).
    website_bypass: CriticalSection,
    account: Mutex<Option<Arc<Account>>>,
    http: ModuleHttp,
    active_task_count: AtomicI32,
    settings: OnceLock<Arc<dyn ModuleSettingsStore>>,
}

impl Module {
    /// A module created by `NewWebsiteModule()` in `file`'s `Init`.
    pub(crate) fn new(file: PathBuf) -> Self {
        Module {
            def: RwLock::new(ModuleDef::new(file)),
            storage: Mutex::default(),
            guardian: Arc::default(),
            website_bypass: CriticalSection::default(),
            account: Mutex::default(),
            http: ModuleHttp::default(),
            active_task_count: AtomicI32::new(0),
            settings: OnceLock::new(),
        }
    }

    /// A copy of the module's current properties.
    pub fn def(&self) -> ModuleDef {
        self.def_read().clone()
    }

    /// The limits the module declares, as its `MODULE` object holds them now. The user's
    /// overrides and the global limits apply on top in `fmd_core::settings::effective_limits`,
    /// with FMD2's precedence (`GetMaxTaskLimit`, `GetMaxThreadPerTaskLimit`,
    /// baseunits/WebsiteModules.pas:398-412).
    pub fn limits(&self) -> ModuleLimits {
        let def = self.def_read();
        ModuleLimits {
            max_task_limit: def.max_task_limit,
            max_thread_per_task_limit: def.max_thread_per_task_limit,
            max_connection_limit: def.max_connection_limit,
        }
    }

    /// Sets `MODULE.CurrentDirectoryIndex`, as the update-list manager does before it walks a
    /// directory's pages (baseunits/uUpdateThread.pas:702).
    pub fn set_current_directory_index(&self, index: i32) {
        self.def_write().current_directory_index = index;
    }

    /// The module's shared HTTP state: its cookie jar and connection queue.
    pub fn http(&self) -> &ModuleHttp {
        &self.http
    }

    /// The module's `Guardian` critical section.
    pub fn guardian(&self) -> &Arc<CriticalSection> {
        &self.guardian
    }

    /// The lock that serialises the module's anti-bot bypasses (`TWebsiteBypass.Guardian`,
    /// baseunits/lua/LuaWebsiteBypass.pas:14-20, taken at :161).
    pub(crate) fn website_bypass_guard(&self) -> &CriticalSection {
        &self.website_bypass
    }

    /// The module's account, present while `AccountSupport` is true.
    pub fn account(&self) -> Option<Arc<Account>> {
        lock(&self.account).clone()
    }

    /// Changes the account's fields with `change`, under the account's lock so a concurrent
    /// write from Lua is not lost, and writes them to the settings store. Does nothing when the
    /// module has no account.
    pub fn update_account(
        &self,
        change: impl FnOnce(&mut AccountState),
    ) -> Result<(), SettingsStoreError> {
        let Some(account) = self.account() else {
            return Ok(());
        };
        change(&mut lock(&account.state));
        self.save_account()
    }

    /// Writes the account to the settings store, after a Lua call or the host changed it.
    fn save_account(&self) -> Result<(), SettingsStoreError> {
        let id = self.def_read().id.clone();
        match (self.settings(), self.account()) {
            (Some(store), Some(account)) if !id.is_empty() => {
                store.set_account(&id, &account.state())
            }
            _ => Ok(()),
        }
    }

    /// The value of `MODULE.Storage[name]`.
    pub fn storage_value(&self, name: &str) -> Vec<u8> {
        lock(&self.storage).values.value(name.as_bytes()).to_vec()
    }

    /// The value `MODULE.GetOption(name)` returns: the stored value when the settings store
    /// holds one of the option's type, else the declared default; `None` for an undeclared
    /// option.
    ///
    /// FMD2 keeps the value in the option itself, set to the default by `AddOption*` and
    /// overwritten when the settings load (baseunits/lua/LuaWebsiteModules.pas:921-949,
    /// baseunits/WebsiteModules.pas:545-633).
    pub fn option_value(&self, name: &str) -> Result<Option<OptionValue>, SettingsStoreError> {
        let (id, option) = {
            let def = self.def_read();
            match def.option(name) {
                Some(option) => (def.id.clone(), option.clone()),
                None => return Ok(None),
            }
        };
        let stored = match self.settings() {
            Some(store) if !id.is_empty() && !option.settings_key().is_empty() => {
                store.option(&id, &option.settings_key())?
            }
            _ => None,
        };
        Ok(Some(match (option.kind, stored) {
            (OptionKind::CheckBox { .. }, Some(v @ OptionValue::Bool(_)))
            | (OptionKind::Edit { .. }, Some(v @ OptionValue::Text(_)))
            | (
                OptionKind::SpinEdit { .. } | OptionKind::ComboBox { .. },
                Some(v @ OptionValue::Integer(_)),
            ) => v,
            (OptionKind::CheckBox { default }, _) => OptionValue::Bool(default),
            (OptionKind::Edit { default }, _) => OptionValue::Text(default),
            (OptionKind::SpinEdit { default } | OptionKind::ComboBox { default, .. }, _) => {
                OptionValue::Integer(default)
            }
        }))
    }

    fn settings(&self) -> Option<&Arc<dyn ModuleSettingsStore>> {
        self.settings.get()
    }

    /// Attaches the settings store the module's options, cookies and account live in, loading
    /// the saved cookies into its jar and the saved account into its `Account`, as FMD2 loads
    /// `modules.json` after the scan.
    fn attach_settings(&self, store: Arc<dyn ModuleSettingsStore>) -> Result<(), String> {
        let id = self.def_read().id.clone();
        if let Some(account) = self.account()
            && let Some(mut saved) = store.account(&id).map_err(|e| e.to_string())?
        {
            // A check that never finished is unknown again (baseunits/WebsiteModules.pas:612-613).
            if saved.status == AccountState::CHECKING {
                saved.status = AccountState::UNKNOWN;
            }
            account.set_state(saved);
        }
        if let Some(cookies) = store.cookies(&id).map_err(|e| e.to_string())? {
            self.http
                .cookies()
                .load_json(&cookies)
                .map_err(|e| format!("saved cookies of module {id}: {e}"))?;
        }
        // A module is attached once, right after its `Init`.
        let _ = self.settings.set(store);
        Ok(())
    }

    /// Writes the cookie jar to the settings store, after a Lua call changed it.
    fn save_cookies(&self) -> Result<(), SettingsStoreError> {
        let id = self.def_read().id.clone();
        match self.settings() {
            Some(store) if !id.is_empty() => {
                let json = self
                    .http
                    .cookies()
                    .to_json()
                    .map_err(SettingsStoreError::new)?;
                store.set_cookies(&id, &json)
            }
            _ => Ok(()),
        }
    }

    fn def_read(&self) -> std::sync::RwLockReadGuard<'_, ModuleDef> {
        // A poisoned lock still holds a valid definition.
        self.def.read().unwrap_or_else(|e| e.into_inner())
    }

    fn def_write(&self) -> std::sync::RwLockWriteGuard<'_, ModuleDef> {
        self.def.write().unwrap_or_else(|e| e.into_inner())
    }
}

/// Locks `mutex`; a poisoned one still holds valid data.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
