//! Module accounts and logins through `OnLogin` (FMD2's account manager,
//! mangadownloader/forms/frmAccountManager.pas).
//!
//! No automatic login: like FMD2, `OnLogin` only runs on request
//! (mangadownloader/forms/frmAccountManager.pas:271-290); modules needing a session log in from
//! their own callbacks (e.g. lua/modules/ProjectTime.lua:67).
//!
//! Threat model: credentials and cookies are encrypted at rest under `accounts.key` (mode 0600),
//! protecting copies of `app.db` without the key file, not anyone who can read the data
//! directory or the process.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use fmd_lua::{AccountState, JobError, Module, ModuleRegistry, SettingsStoreError, WorkerPool};
use fmd_store::AccountStatus;
use thiserror::Error;
use tokio::sync::broadcast;

/// How far a slow listener may fall behind before it misses changes.
const CHANGES_CAPACITY: usize = 64;

/// The ordinal in `TAccountStatus = (asUnknown, asChecking, asValid, asInvalid)`
/// (baseunits/WebsiteModules.pas:78), as `MODULE.Account.Status` holds it.
pub fn status_ordinal(status: AccountStatus) -> i32 {
    match status {
        AccountStatus::Unknown => 0,
        AccountStatus::Checking => 1,
        AccountStatus::Valid => 2,
        AccountStatus::Invalid => 3,
    }
}

/// Inverse of [`status_ordinal`]; out-of-range values are unknown
/// (baseunits/WebsiteModules.pas:78).
pub fn status_of(ordinal: i32) -> AccountStatus {
    match ordinal {
        1 => AccountStatus::Checking,
        2 => AccountStatus::Valid,
        3 => AccountStatus::Invalid,
        _ => AccountStatus::Unknown,
    }
}

/// A module's account without the password and cookies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    pub module_id: String,
    pub module_name: String,
    pub enabled: bool,
    pub username: String,
    pub has_password: bool,
    pub status: AccountStatus,
}

/// `None` leaves a field as it is.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AccountUpdate {
    pub username: Option<String>,
    pub password: Option<String>,
    pub enabled: Option<bool>,
}

impl std::fmt::Debug for AccountUpdate {
    /// Leaves the credentials out of logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountUpdate")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountChange {
    pub module_id: String,
    pub status: AccountStatus,
}

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("no module {0}")]
    UnknownModule(String),
    #[error("module {0} has no account support")]
    NoAccountSupport(String),
    /// FMD2 only calls `OnLogin` when the module assigned it
    /// (baseunits/lua/LuaWebsiteModules.pas:578-579).
    #[error("module {0} has no login callback")]
    NoLogin(String),
    /// FMD2 ignores a check while one runs (mangadownloader/forms/frmAccountManager.pas:288).
    #[error("the account of module {0} is busy with a login")]
    Checking(String),
    #[error(transparent)]
    Store(#[from] SettingsStoreError),
    #[error(transparent)]
    Pool(#[from] JobError),
}

/// Lists, edits and logs in the accounts of the loaded modules. Every method blocks.
pub struct AccountService {
    /// Read anew for every request so a hot reload is followed.
    current_modules: Box<CurrentModules>,
    pool: Arc<WorkerPool>,
    checking: Mutex<HashSet<String>>,
    changes: broadcast::Sender<AccountChange>,
}

type CurrentModules = dyn Fn() -> Arc<ModuleRegistry> + Send + Sync;

impl AccountService {
    pub fn new(registry: Arc<ModuleRegistry>, pool: Arc<WorkerPool>) -> Self {
        Self::following(move || registry.clone(), pool)
    }

    /// Follows the registry `current` returns at each request.
    pub fn following(
        current: impl Fn() -> Arc<ModuleRegistry> + Send + Sync + 'static,
        pool: Arc<WorkerPool>,
    ) -> Self {
        Self {
            current_modules: Box::new(current),
            pool,
            checking: Mutex::default(),
            changes: broadcast::channel(CHANGES_CAPACITY).0,
        }
    }

    /// The start and end of every login.
    pub fn subscribe(&self) -> broadcast::Receiver<AccountChange> {
        self.changes.subscribe()
    }

    /// By module ID (mangadownloader/forms/frmAccountManager.pas:168-182).
    pub fn list(&self) -> Vec<AccountView> {
        let mut accounts: Vec<AccountView> = (self.current_modules)()
            .modules()
            .iter()
            .filter_map(|m| view(m))
            .collect();
        accounts.sort_by(|a, b| a.module_id.cmp(&b.module_id));
        accounts
    }

    pub fn account(&self, module_id: &str) -> Result<AccountView, AccountError> {
        let module = self.module(module_id)?;
        view(&module).ok_or_else(|| AccountError::NoAccountSupport(module_id.to_owned()))
    }

    /// New credentials make the status unknown; unlike FMD2
    /// (mangadownloader/forms/frmAccountManager.pas:271-277), checking them is up to the caller.
    /// Toggling runs `OnAccountState` (:292-300). Refused while a login runs, so it cannot
    /// overwrite what the login stores.
    pub fn update(
        &self,
        module_id: &str,
        update: AccountUpdate,
    ) -> Result<AccountView, AccountError> {
        let module = self.module(module_id)?;
        let _busy = self.start_check(module_id)?;
        let was_enabled = self.state(&module)?.enabled;
        module.update_account(|state| {
            let mut credentials_changed = false;
            if let Some(username) = update.username {
                credentials_changed |= username != state.username;
                state.username = username;
            }
            if let Some(password) = update.password {
                credentials_changed |= password != state.password;
                state.password = password;
            }
            if let Some(enabled) = update.enabled {
                state.enabled = enabled;
            }
            if credentials_changed {
                state.status = AccountState::UNKNOWN;
            }
        })?;
        if self.state(&module)?.enabled != was_enabled {
            self.account_state(&module, None);
        }
        self.account(module_id)
    }

    /// Resets the account to a new one's state (the module owns it, so it cannot be removed).
    /// Refused while a login runs.
    pub fn delete(&self, module_id: &str) -> Result<AccountView, AccountError> {
        let module = self.module(module_id)?;
        let _busy = self.start_check(module_id)?;
        let was_enabled = self.state(&module)?.enabled;
        module.update_account(|state| *state = AccountState::default())?;
        if was_enabled {
            self.account_state(&module, None);
        }
        self.account(module_id)
    }

    /// `TAccountCheckThread.Execute` (mangadownloader/forms/frmAccountManager.pas:125-136),
    /// then, unlike FMD2, `OnAccountState` so a module like Madokami loads the new cookies. A
    /// status left `asChecking` becomes unknown, as FMD2 does on its next start
    /// (baseunits/WebsiteModules.pas:612-613).
    pub fn login(&self, module_id: &str) -> Result<AccountView, AccountError> {
        let module = self.module(module_id)?;
        // Report missing account support before a missing login.
        let credentials = self.state(&module)?;
        if module.def().on_login.is_none() {
            return Err(AccountError::NoLogin(module_id.to_owned()));
        }
        let _checking = self.start_check(module_id)?;
        module.update_account(|state| state.status = AccountState::CHECKING)?;
        self.announce(module_id, AccountStatus::Checking);

        let affinity = self.pool.affinity();
        let login = self.pool.on(&module).with_affinity(affinity).login().wait();
        match login {
            Ok(_) => {}
            Err(JobError::Callback(e)) => {
                let error = redact(&e.to_string(), &credentials);
                tracing::warn!(target: "fmd_core::accounts", "login of module {module_id}: {error}");
            }
            Err(e) => {
                self.finish_check(&module, module_id)?;
                return Err(e.into());
            }
        }
        self.account_state(&module, Some(affinity));
        self.finish_check(&module, module_id)?;
        self.account(module_id)
    }

    /// A status still `asChecking` becomes unknown.
    fn finish_check(&self, module: &Arc<Module>, module_id: &str) -> Result<(), AccountError> {
        module.update_account(|state| {
            if state.status == AccountState::CHECKING {
                state.status = AccountState::UNKNOWN;
            }
        })?;
        let status = status_of(self.state(module)?.status);
        self.announce(module_id, status);
        Ok(())
    }

    /// `DoAccountState` (baseunits/lua/LuaWebsiteModules.pas:431-447): the result is unused and
    /// a failure only logged.
    fn account_state(&self, module: &Arc<Module>, affinity: Option<fmd_lua::Affinity>) {
        if module.def().on_account_state.is_none() {
            return;
        }
        let mut caller = self.pool.on(module);
        if let Some(affinity) = affinity {
            caller = caller.with_affinity(affinity);
        }
        if let Err(e) = caller.account_state().wait() {
            let id = module.def().id;
            let error = match module.account() {
                Some(account) => redact(&e.to_string(), &account.state()),
                None => e.to_string(),
            };
            tracing::warn!(target: "fmd_core::accounts", "account state of module {id}: {error}");
        }
    }

    fn announce(&self, module_id: &str, status: AccountStatus) {
        let _ = self.changes.send(AccountChange {
            module_id: module_id.to_owned(),
            status,
        });
    }

    /// Marks `module_id` busy until the guard drops.
    fn start_check(&self, module_id: &str) -> Result<CheckGuard<'_>, AccountError> {
        let mut checking = self.checking.lock().unwrap_or_else(PoisonError::into_inner);
        if !checking.insert(module_id.to_owned()) {
            return Err(AccountError::Checking(module_id.to_owned()));
        }
        Ok(CheckGuard {
            service: self,
            module_id: module_id.to_owned(),
        })
    }

    fn module(&self, module_id: &str) -> Result<Arc<Module>, AccountError> {
        (self.current_modules)()
            .get(module_id)
            .cloned()
            .ok_or_else(|| AccountError::UnknownModule(module_id.to_owned()))
    }

    fn state(&self, module: &Module) -> Result<AccountState, AccountError> {
        module
            .account()
            .map(|a| a.state())
            .ok_or_else(|| AccountError::NoAccountSupport(module.def().id))
    }
}

struct CheckGuard<'a> {
    service: &'a AccountService,
    module_id: String,
}

impl Drop for CheckGuard<'_> {
    fn drop(&mut self) {
        self.service
            .checking
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.module_id);
    }
}

/// Masks the account's credentials and cookies in a module error before it is logged.
fn redact(message: &str, account: &AccountState) -> String {
    let mut message = message.to_owned();
    for secret in [&account.password, &account.cookies, &account.username] {
        if !secret.is_empty() {
            message = message.replace(secret.as_str(), "***");
        }
    }
    message
}

fn view(module: &Module) -> Option<AccountView> {
    let state = module.account()?.state();
    let def = module.def();
    Some(AccountView {
        module_id: def.id,
        module_name: def.name,
        enabled: state.enabled,
        has_password: !state.password.is_empty(),
        username: state.username,
        status: status_of(state.status),
    })
}
