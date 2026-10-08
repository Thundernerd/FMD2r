//! Module accounts and login through `AccountService`, over the ticket's fixture module
//! (docs/tickets/T31-accounts-login.md, "Seams under test").
//!
//! Expected values come from FMD2's account check (`TAccountCheckThread.Execute`,
//! mangadownloader/forms/frmAccountManager.pas:124-134), `DoLogin`/`DoAccountState`
//! (baseunits/lua/LuaWebsiteModules.pas:412-447) and the account fields FMD2 saves encrypted
//! (baseunits/WebsiteModules.pas:600-615, :665-675).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use fmd_core::accounts::{AccountError, AccountService, AccountUpdate};
use fmd_core::modules::StoreModuleSettings;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_store::{AccountStatus, AppDb, Cipher, KeyFileCipher};
use tempfile::TempDir;

const FIXTURE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'fixture'
  m.Name = 'Fixture'
  m.RootURL = 'https://fixture.example'
  m.AccountSupport = true
  m.OnLogin = 'Login'
  m.OnAccountState = 'AccountState'
  m.OnGetInfo = 'GetInfo'
end

function Login()
  if MODULE.Account.Username == 'slow' then sleep(500) end
  if MODULE.Account.Username == 'u' and MODULE.Account.Password == 'p' then
    MODULE.Account.Cookies = 'sid=1'; MODULE.Account.Status = asValid; return true
  end
  MODULE.Account.Status = asInvalid; return false
end

-- Records what it saw, so the test can tell it ran after Login.
function AccountState()
  MODULE.Storage['state_saw'] = tostring(MODULE.Account.Status)
  return true
end

-- A module that logs itself in from another callback, like Madokami's CheckAuth.
function GetInfo()
  MODULE.Account.Cookies = 'sid=from-info'
  MODULE.Account.Status = asValid
  return no_error
end
"#;

/// Never reaches the network: every request fails.
struct Offline;

impl Transport for Offline {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("offline".into())) })
    }
}

struct Harness {
    dir: TempDir,
    db: AppDb,
    cipher: Arc<dyn Cipher>,
    registry: Arc<ModuleRegistry>,
    pool: Arc<WorkerPool>,
    service: AccountService,
}

impl Harness {
    fn new() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let lua = dir.path().join("lua");
        std::fs::create_dir_all(lua.join("modules")).unwrap();
        std::fs::write(lua.join("modules/Fixture.lua"), FIXTURE).unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let cipher: Arc<dyn Cipher> =
            Arc::new(KeyFileCipher::open_or_create(dir.path().join("accounts.key")).unwrap());
        Self::load(dir, db, cipher)
    }

    /// Loads the modules again over the same database, as a restart would.
    fn restart(self) -> Harness {
        let Harness {
            dir, db, cipher, ..
        } = self;
        Self::load(dir, db, cipher)
    }

    fn load(dir: TempDir, db: AppDb, cipher: Arc<dyn Cipher>) -> Harness {
        let lua = dir.path().join("lua");
        let store = Arc::new(StoreModuleSettings::new(db.clone(), cipher.clone()));
        let report = ModuleRegistry::load_dir_with(&lua, store);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let registry = Arc::new(report.registry);
        let mut config = PoolConfig::new(HttpClient::with_transport(Arc::new(Offline)).unwrap());
        config.threads = 1;
        config.lua_dir = lua;
        let pool = Arc::new(WorkerPool::new(config).unwrap());
        let service = AccountService::new(registry.clone(), pool.clone());
        Harness {
            dir,
            db,
            cipher,
            registry,
            pool,
            service,
        }
    }

    fn set_credentials(&self, username: &str, password: &str) {
        self.service
            .update(
                "fixture",
                AccountUpdate {
                    username: Some(username.into()),
                    password: Some(password.into()),
                    enabled: Some(true),
                },
            )
            .unwrap();
    }

    /// The `accounts` row as raw bytes, read past the repository.
    fn raw_row(&self) -> Vec<u8> {
        let raw = rusqlite::Connection::open(self.dir.path().join("app.db")).unwrap();
        let (user, pass, cookies): (Vec<u8>, Vec<u8>, Vec<u8>) = raw
            .query_row(
                "SELECT username, password, cookies FROM accounts WHERE module_id = 'fixture'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        [user, pass, cookies].concat()
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn login_with_the_right_credentials_is_valid_and_persists_encrypted_cookies() {
    let h = Harness::new();
    h.set_credentials("u", "p");

    assert_eq!(
        h.service.login("fixture").unwrap().status,
        AccountStatus::Valid
    );

    let stored = h.db.accounts(h.cipher.as_ref()).get("fixture").unwrap();
    let stored = stored.unwrap();
    assert_eq!(stored.status, AccountStatus::Valid);
    assert_eq!(stored.cookies, "sid=1");
    assert!(stored.enabled);
    assert!(
        !contains(&h.raw_row(), "sid=1"),
        "cookies stored in plaintext"
    );
}

#[test]
fn login_with_a_wrong_password_is_invalid() {
    let h = Harness::new();
    h.set_credentials("u", "wrong");

    assert_eq!(
        h.service.login("fixture").unwrap().status,
        AccountStatus::Invalid
    );
    assert_eq!(
        h.service.account("fixture").unwrap().status,
        AccountStatus::Invalid
    );
}

#[test]
fn account_state_runs_after_login_and_sees_its_status() {
    let h = Harness::new();
    h.set_credentials("u", "p");
    h.service.login("fixture").unwrap();

    let module = h.registry.get("fixture").unwrap();
    // asValid is 2 (baseunits/WebsiteModules.pas:78).
    assert_eq!(module.storage_value("state_saw"), b"2");
}

#[test]
fn a_login_announces_checking_then_its_result() {
    let h = Harness::new();
    h.set_credentials("u", "p");
    let mut changes = h.service.subscribe();

    h.service.login("fixture").unwrap();

    let statuses: Vec<_> = std::iter::from_fn(|| changes.try_recv().ok())
        .map(|c| (c.module_id, c.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("fixture".to_owned(), AccountStatus::Checking),
            ("fixture".to_owned(), AccountStatus::Valid)
        ]
    );
}

#[test]
fn what_a_module_writes_to_its_account_survives_a_restart() {
    let h = Harness::new();
    h.set_credentials("u", "p");
    let module = h.registry.get("fixture").unwrap().clone();
    h.pool.on(&module).get_info("/m").wait().unwrap();
    drop(module);

    let h = h.restart();

    let account = h.service.account("fixture").unwrap();
    assert_eq!(account.status, AccountStatus::Valid);
    assert_eq!(account.username, "u");
    assert!(account.has_password);
    let state = h
        .registry
        .get("fixture")
        .unwrap()
        .account()
        .unwrap()
        .state();
    assert_eq!(state.cookies, "sid=from-info");
    assert_eq!(state.password, "p");
    assert!(state.enabled);
}

#[test]
fn the_api_view_never_holds_the_password() {
    let h = Harness::new();
    h.set_credentials("u", "p");

    let listed = h.service.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].module_id, "fixture");
    assert_eq!(listed[0].module_name, "Fixture");
    assert!(listed[0].has_password);
    assert!(!format!("{:?}", listed[0]).contains("\"p\""));
}

#[test]
fn deleting_an_account_clears_it() {
    let h = Harness::new();
    h.set_credentials("u", "p");
    h.service.login("fixture").unwrap();

    let account = h.service.delete("fixture").unwrap();

    assert_eq!(account.username, "");
    assert!(!account.has_password);
    assert!(!account.enabled);
    assert_eq!(account.status, AccountStatus::Unknown);
    let stored = h.db.accounts(h.cipher.as_ref()).get("fixture").unwrap();
    assert!(stored.is_none_or(|a| a.cookies.is_empty() && a.password.is_empty()));
}

#[test]
fn unknown_modules_are_refused() {
    let h = Harness::new();
    assert!(matches!(
        h.service.login("nope"),
        Err(AccountError::UnknownModule(_))
    ));
}

#[test]
fn an_edit_while_a_login_runs_is_refused_and_cannot_undo_it() {
    let h = Harness::new();
    h.set_credentials("slow", "p");
    std::thread::scope(|scope| {
        let login = scope.spawn(|| h.service.login("fixture"));
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(matches!(
            h.service.update("fixture", AccountUpdate::default()),
            Err(AccountError::Checking(_))
        ));
        assert!(matches!(
            h.service.login("fixture"),
            Err(AccountError::Checking(_))
        ));
        assert_eq!(
            login.join().unwrap().unwrap().status,
            AccountStatus::Invalid
        );
    });
}
