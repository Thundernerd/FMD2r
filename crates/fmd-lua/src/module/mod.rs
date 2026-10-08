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

/// The fields of an [`Account`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountState {
    pub enabled: bool,
    pub username: String,
    pub password: String,
    /// `TAccountStatus` as its ordinal: 0 unknown, 1 checking, 2 valid, 3 invalid.
    pub status: i32,
    pub cookies: String,
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
    account: Mutex<Option<Arc<Account>>>,
    http: ModuleHttp,
    active_task_count: AtomicI32,
    settings: OnceLock<Arc<dyn ModuleSettingsStore>>,
}

impl Module {
    /// A module created by `NewWebsiteModule()` in `file`'s `Init`.
    fn new(file: PathBuf) -> Self {
        Module {
            def: RwLock::new(ModuleDef::new(file)),
            storage: Mutex::default(),
            guardian: Arc::default(),
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

    /// The module's shared HTTP state: its cookie jar and connection queue.
    pub fn http(&self) -> &ModuleHttp {
        &self.http
    }

    /// The module's `Guardian` critical section.
    pub fn guardian(&self) -> &Arc<CriticalSection> {
        &self.guardian
    }

    /// The module's account, present while `AccountSupport` is true.
    pub fn account(&self) -> Option<Arc<Account>> {
        lock(&self.account).clone()
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

    /// Attaches the settings store the module's options and cookies live in, loading the saved
    /// cookies into its jar, as FMD2 loads `modules.json` after the scan.
    fn attach_settings(&self, store: Arc<dyn ModuleSettingsStore>) -> Result<(), String> {
        let id = self.def_read().id.clone();
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
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
