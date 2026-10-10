//! Where a module's option values and cookies persist (FMD2's `modules.json`,
//! baseunits/WebsiteModules.pas:545-633 loads options and cookies, :635 saves them).

use std::collections::HashMap;
use std::sync::Mutex;

use super::AccountState;

/// A stored option value, typed like the option kinds (`TWebsiteOptionType`,
/// baseunits/WebsiteModules.pas:68).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionValue {
    /// A checkbox.
    Bool(bool),
    /// An edit field.
    Text(String),
    /// A spin edit's number or a combo box's selected index.
    Integer(i32),
}

/// A settings store failed to read or write.
#[derive(Debug, thiserror::Error)]
#[error("module settings store: {0}")]
pub struct SettingsStoreError(Box<dyn std::error::Error + Send + Sync>);

impl SettingsStoreError {
    pub fn new(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        SettingsStoreError(error.into())
    }
}

/// Per-module option values, cookies and accounts, keyed by module ID. Options are keyed by
/// [`ModuleOption::settings_key`](super::ModuleOption::settings_key) as in FMD2's
/// `modules.json`; cookies are the jar as JSON ([`fmd_http::CookieJar::to_json`]).
pub trait ModuleSettingsStore: Send + Sync {
    fn option(
        &self,
        module_id: &str,
        name: &str,
    ) -> Result<Option<OptionValue>, SettingsStoreError>;
    fn set_option(
        &self,
        module_id: &str,
        name: &str,
        value: OptionValue,
    ) -> Result<(), SettingsStoreError>;
    fn cookies(&self, module_id: &str) -> Result<Option<String>, SettingsStoreError>;
    fn set_cookies(&self, module_id: &str, cookies: &str) -> Result<(), SettingsStoreError>;
    /// FMD2 keeps it in `modules.json` (baseunits/WebsiteModules.pas:600-615).
    fn account(&self, module_id: &str) -> Result<Option<AccountState>, SettingsStoreError>;
    /// (baseunits/WebsiteModules.pas:665-675). Keep credentials and cookies encrypted, as FMD2
    /// does with `EncryptString`.
    fn set_account(
        &self,
        module_id: &str,
        account: &AccountState,
    ) -> Result<(), SettingsStoreError>;
}

/// A [`ModuleSettingsStore`] that keeps everything in memory.
#[derive(Default)]
pub struct MemorySettingsStore {
    options: Mutex<HashMap<(String, String), OptionValue>>,
    cookies: Mutex<HashMap<String, String>>,
    accounts: Mutex<HashMap<String, AccountState>>,
}

impl MemorySettingsStore {
    pub fn new() -> Self {
        MemorySettingsStore::default()
    }
}

impl ModuleSettingsStore for MemorySettingsStore {
    fn option(
        &self,
        module_id: &str,
        name: &str,
    ) -> Result<Option<OptionValue>, SettingsStoreError> {
        Ok(super::lock(&self.options)
            .get(&(module_id.to_owned(), name.to_owned()))
            .cloned())
    }

    fn set_option(
        &self,
        module_id: &str,
        name: &str,
        value: OptionValue,
    ) -> Result<(), SettingsStoreError> {
        super::lock(&self.options).insert((module_id.to_owned(), name.to_owned()), value);
        Ok(())
    }

    fn cookies(&self, module_id: &str) -> Result<Option<String>, SettingsStoreError> {
        Ok(super::lock(&self.cookies).get(module_id).cloned())
    }

    fn set_cookies(&self, module_id: &str, cookies: &str) -> Result<(), SettingsStoreError> {
        super::lock(&self.cookies).insert(module_id.to_owned(), cookies.to_owned());
        Ok(())
    }

    fn account(&self, module_id: &str) -> Result<Option<AccountState>, SettingsStoreError> {
        Ok(super::lock(&self.accounts).get(module_id).cloned())
    }

    fn set_account(
        &self,
        module_id: &str,
        account: &AccountState,
    ) -> Result<(), SettingsStoreError> {
        super::lock(&self.accounts).insert(module_id.to_owned(), account.clone());
        Ok(())
    }
}
