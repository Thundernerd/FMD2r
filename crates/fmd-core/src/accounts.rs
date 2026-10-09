//! Module accounts: listing and editing the accounts of modules with `AccountSupport`, and logging
//! in through the module's `OnLogin` (FMD2's account manager,
//! mangadownloader/forms/frmAccountManager.pas).
//!
//! The module's `MODULE.Account` is the account: [`AccountService`] reads and writes it, and every
//! change, from here or from Lua, is written to the module's settings store
//! ([`crate::modules::StoreModuleSettings`]), which keeps it in `app.db` encrypted.
//!
//! # When logins run
//!
//! FMD2 runs `OnLogin` only from its account manager: when the user checks an account or saves
//! new credentials (mangadownloader/forms/frmAccountManager.pas:271-290). It never logs in before
//! a task or an info call; modules that need a session call their own login function from their
//! callbacks instead (e.g. `CheckAuth` in lua/modules/Madokami.lua, lua/modules/ProjectTime.lua:67).
//! FMD2r does the same, so there is no automatic login here: what such a callback writes to
//! `MODULE.Account` is persisted like any other change.
//!
//! # Threat model
//!
//! Usernames, passwords and cookies are encrypted at rest with XChaCha20-Poly1305 under a random
//! key in the data directory (`accounts.key`, mode 0600). That protects copies of `app.db`, such
//! as backups or a database shared for debugging, as long as the key file does not travel with
//! them. It does not protect against anyone who can read the data directory or the running
//! process: they hold the key, and the server needs the plaintext to log in.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use fmd_lua::{AccountState, JobError, Module, ModuleRegistry, SettingsStoreError, WorkerPool};
use fmd_store::AccountStatus;
use thiserror::Error;
use tokio::sync::broadcast;

/// How many account changes a slow listener may fall behind before it misses some.
const CHANGES_CAPACITY: usize = 64;

/// The ordinal of `status` in `TAccountStatus = (asUnknown, asChecking, asValid, asInvalid)`
/// (baseunits/WebsiteModules.pas:78), the value `MODULE.Account.Status` holds.
pub fn status_ordinal(status: AccountStatus) -> i32 {
    match status {
        AccountStatus::Unknown => 0,
        AccountStatus::Checking => 1,
        AccountStatus::Valid => 2,
        AccountStatus::Invalid => 3,
    }
}

/// The status `MODULE.Account.Status` holds (baseunits/WebsiteModules.pas:78). A value outside
/// `TAccountStatus` counts as unknown.
pub fn status_of(ordinal: i32) -> AccountStatus {
    match ordinal {
        1 => AccountStatus::Checking,
        2 => AccountStatus::Valid,
        3 => AccountStatus::Invalid,
        _ => AccountStatus::Unknown,
    }
}

/// A module's account as the UI sees it: the password and cookies are never part of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    pub module_id: String,
    pub module_name: String,
    pub enabled: bool,
    pub username: String,
    /// Whether a password is set.
    pub has_password: bool,
    pub status: AccountStatus,
}

/// The fields of an account to change; `None` leaves a field as it is.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AccountUpdate {
    pub username: Option<String>,
    pub password: Option<String>,
    pub enabled: Option<bool>,
}

impl std::fmt::Debug for AccountUpdate {
    /// Leaves the credentials out, so they never reach a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountUpdate")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

/// An account's status changed: a login started or finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountChange {
    pub module_id: String,
    pub status: AccountStatus,
}

/// Why an account request failed.
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
    /// A login or an edit of the account is running. FMD2 ignores a check while one runs
    /// (mangadownloader/forms/frmAccountManager.pas:288).
    #[error("the account of module {0} is busy with a login")]
    Checking(String),
    #[error(transparent)]
    Store(#[from] SettingsStoreError),
    #[error(transparent)]
    Pool(#[from] JobError),
}

/// Lists, edits and logs in the accounts of the loaded modules.
///
/// Every method blocks (on the store, and for [`login`](Self::login) on the module's callbacks):
/// call them from a blocking thread, never from inside a tokio runtime.
pub struct AccountService {
    /// The loaded modules, read anew for every request so a hot reload is followed.
    registry: Box<CurrentModules>,
    pool: Arc<WorkerPool>,
    /// Modules whose login is running.
    checking: Mutex<HashSet<String>>,
    changes: broadcast::Sender<AccountChange>,
}

/// Returns the registry of the modules loaded now.
type CurrentModules = dyn Fn() -> Arc<ModuleRegistry> + Send + Sync;

impl AccountService {
    /// The accounts of the modules in `registry`.
    pub fn new(registry: Arc<ModuleRegistry>, pool: Arc<WorkerPool>) -> Self {
        Self::following(move || registry.clone(), pool)
    }

    /// The accounts of the modules `current` returns at the time of each request, e.g. the
    /// registry the module updater last reloaded.
    pub fn following(
        current: impl Fn() -> Arc<ModuleRegistry> + Send + Sync + 'static,
        pool: Arc<WorkerPool>,
    ) -> Self {
        Self {
            registry: Box::new(current),
            pool,
            checking: Mutex::default(),
            changes: broadcast::channel(CHANGES_CAPACITY).0,
        }
    }

    /// Status changes from now on: the start and the end of every login.
    pub fn subscribe(&self) -> broadcast::Receiver<AccountChange> {
        self.changes.subscribe()
    }

    /// The accounts of every module with account support, by module ID
    /// (mangadownloader/forms/frmAccountManager.pas:168-182).
    pub fn list(&self) -> Vec<AccountView> {
        let mut accounts: Vec<AccountView> = (self.registry)()
            .modules()
            .iter()
            .filter_map(|m| view(m))
            .collect();
        accounts.sort_by(|a, b| a.module_id.cmp(&b.module_id));
        accounts
    }

    /// The account of module `module_id`.
    pub fn account(&self, module_id: &str) -> Result<AccountView, AccountError> {
        let module = self.module(module_id)?;
        view(&module).ok_or_else(|| AccountError::NoAccountSupport(module_id.to_owned()))
    }

    /// Changes the account's fields. New credentials make the status unknown until the next
    /// login; FMD2 checks them right away instead (mangadownloader/forms/frmAccountManager.pas:
    /// 271-277), which here is up to the caller. Turning the account on or off runs
    /// `OnAccountState` when the module has one, as ticking it in FMD2's account list does
    /// (mangadownloader/forms/frmAccountManager.pas:292-300). Refused while a login runs, so it
    /// cannot overwrite what the login stores.
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

    /// Clears the account's credentials and cookies and turns it off. FMD2 cannot delete an
    /// account (the module owns it); this leaves it as a new one is. Refused while a login runs.
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

    /// Logs in. Like FMD2's account check (`TAccountCheckThread.Execute`,
    /// mangadownloader/forms/frmAccountManager.pas:125-136), the status turns `asChecking`, then
    /// `OnLogin` runs with a new `HTTP` session for the module. Then, as the ticket asks (FMD2's
    /// check does not), `OnAccountState` runs when the module has one, so a module like
    /// Madokami loads the new cookies. The status the module leaves is stored, announced and
    /// returned with the account. A login whose callback fails is logged and leaves the status
    /// the module set; one that leaves it `asChecking` makes it unknown, as FMD2 does on its next
    /// start (baseunits/WebsiteModules.pas:612-613).
    pub fn login(&self, module_id: &str) -> Result<AccountView, AccountError> {
        let module = self.module(module_id)?;
        // A module without account support is reported as such before a missing login.
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

    /// Ends a check: a status still `asChecking` becomes unknown, and the result is announced.
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

    /// Runs `OnAccountState` when the module has one (`DoAccountState`,
    /// baseunits/lua/LuaWebsiteModules.pas:431-447). Like FMD2, its result is not used and a
    /// failure is only logged.
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
        // Nobody listening is fine.
        let _ = self.changes.send(AccountChange {
            module_id: module_id.to_owned(),
            status,
        });
    }

    /// Marks `module_id` as busy (a login or an edit running) until the guard drops.
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
        (self.registry)()
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

/// Removes its module from the modules being checked when dropped.
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

/// `message` with the account's username, password and cookies masked, for a module error that
/// quotes them, so credentials never reach the log.
fn redact(message: &str, account: &AccountState) -> String {
    let mut message = message.to_owned();
    for secret in [&account.password, &account.cookies, &account.username] {
        if !secret.is_empty() {
            message = message.replace(secret.as_str(), "***");
        }
    }
    message
}

/// The account of `module`, when it supports accounts.
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
