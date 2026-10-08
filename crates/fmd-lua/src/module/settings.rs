//! Where a module's option values and cookies persist (FMD2's `modules.json`,
//! baseunits/WebsiteModules.pas:545-633 loads options and cookies, :635 saves them).

use std::collections::HashMap;
use std::sync::Mutex;

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
    /// Wraps the store's own error.
    pub fn new(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        SettingsStoreError(error.into())
    }
}

/// Per-module option values and cookies, keyed by module ID. Options are keyed by the name the
/// module declared them with. Cookies are the module's cookie jar as JSON
/// ([`fmd_http::CookieJar::to_json`]).
pub trait ModuleSettingsStore: Send + Sync {
    /// The stored value of option `name`, if any.
    fn option(
        &self,
        module_id: &str,
        name: &str,
    ) -> Result<Option<OptionValue>, SettingsStoreError>;
    /// Stores the value of option `name`.
    fn set_option(
        &self,
        module_id: &str,
        name: &str,
        value: OptionValue,
    ) -> Result<(), SettingsStoreError>;
    /// The stored cookie jar, if any.
    fn cookies(&self, module_id: &str) -> Result<Option<String>, SettingsStoreError>;
    /// Stores the cookie jar.
    fn set_cookies(&self, module_id: &str, cookies: &str) -> Result<(), SettingsStoreError>;
}

/// A [`ModuleSettingsStore`] that keeps everything in memory.
#[derive(Default)]
pub struct MemorySettingsStore {
    options: Mutex<HashMap<(String, String), OptionValue>>,
    cookies: Mutex<HashMap<String, String>>,
}

impl MemorySettingsStore {
    /// An empty store.
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
}
