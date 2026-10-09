//! `import()` over a `userdata` directory a real FMD2 binary wrote (fixtures/fmd2/userdata; see
//! fixtures/README.md for how it was made), so the account and proxy credentials are ciphertext
//! FMD2's own `EncryptString` produced (baseunits/uBaseUnit.pas:1559-1573).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;

use common::App;
use fmd_core::settings::{ModuleOverrides, ProxyOverrideType, ProxyType, SettingsService};
use fmd_import::{ImportOptions, SkipReason};
use fmd_store::AccountStatus;
use serde_json::json;

/// Com-X, the fixture's module with an account (`m.AccountSupport`, fixtures/lua/modules/ComX.lua).
const COMX: &str = "bdf2eb4381a7403ca93d144b9dbc0d0a";
const MANGADEX: &str = "d07c9c2425764da8ba056505f57cf40c";

/// Imports a copy of the fixture, so SQLite never touches the committed files.
fn import_fixture(app: &App) -> fmd_import::ImportReport {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fmd2/userdata");
    let userdata = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), userdata.path().join(entry.file_name())).unwrap();
    }
    fmd_import::import(
        userdata.path(),
        &app.db,
        &app.cipher,
        &ImportOptions::default(),
    )
    .unwrap()
}

#[test]
fn the_account_fmd2_encrypted_is_imported_in_plaintext() {
    let app = App::new();

    let report = import_fixture(&app);

    assert_eq!(report.accounts.imported, 1, "{:?}", report.accounts);
    let account = app.db.accounts(&app.cipher).get(COMX).unwrap().unwrap();
    assert_eq!(account.username, "fixture-user@example.test");
    assert_eq!(account.password, "not-a-real-password");
    // FMD2 decrypted and re-encrypted this one (baseunits/WebsiteModules.pas:609-611, 671-673)
    // without stopping at the NUL.
    assert_eq!(account.cookies, "sid=1\0tail");
    assert!(account.enabled);
    assert_eq!(account.status, AccountStatus::Valid);
}

#[test]
fn the_proxy_credentials_fmd2_encrypted_are_imported_in_plaintext() {
    let app = App::new();

    let report = import_fixture(&app);

    assert!(report.settings.found);
    // Everything else FMD2 wrote equals FMD2r's defaults, which already exist.
    assert!(
        report
            .settings
            .skipped
            .iter()
            .all(|s| s.reason == SkipReason::AlreadyExists),
        "{:?}",
        report.settings
    );
    let s = SettingsService::load(app.db.clone()).unwrap().get();
    let p = &s.connections.proxy;
    assert!(p.enabled);
    assert_eq!(p.kind, ProxyType::Http);
    assert_eq!(p.host, "proxy.example.test");
    assert_eq!(p.port, Some(1080));
    assert_eq!(p.username, "proxy-user");
    assert_eq!(p.password, "pr0xy päss €");
    assert_eq!(s.connections.max_parallel_tasks, 3);
}

#[test]
fn the_module_settings_fmd2_wrote_are_imported() {
    let app = App::new();

    import_fixture(&app);

    let repo = app.db.module_settings();
    let comx = ModuleOverrides::load(&repo, COMX).unwrap();
    assert!(comx.enabled);
    assert_eq!(comx.http.user_agent, "FMD2r-fixture/1.0");
    assert_eq!(
        (comx.limits.max_task_limit, comx.limits.max_connection_limit),
        (2, 4)
    );
    let dex = ModuleOverrides::load(&repo, MANGADEX).unwrap();
    assert_eq!(dex.http.proxy.kind, ProxyOverrideType::Http);
    assert_eq!(dex.http.proxy.host, "module-proxy.test");
    assert_eq!(dex.http.proxy.port, "3128");
    assert_eq!(dex.http.proxy.username, "mod-user");
    assert_eq!(dex.http.proxy.password, "mod-pass");
    assert_eq!(dex.options["lualang"], json!(3));
    assert_eq!(dex.options["luashowscangroup"], json!(true));
}
