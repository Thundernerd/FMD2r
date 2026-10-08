// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::{App, Fmd2};
use fmd_core::settings::{ModuleOverrides, ProxyOverrideType};
use fmd_store::{AccountStatus, Cipher};
use serde_json::json;

const MANGADEX: &str = "d07c9c2425764da8ba056505f57cf40c";
const PLAIN: &str = "4c6f3ad282334f1499106881f28e6310";

/// `modules.json` as `TWebsiteModules.SaveToFile` writes it (baseunits/WebsiteModules.pas:640-696):
/// `Settings` streamed from `TWebsiteModuleSettings` (baseunits/WebsiteModulesSettings.pas:62-91),
/// option values by name, the account with `EncryptString`ed fields and persistent cookies.
fn modules_json() -> String {
    // 'pcjhSQpguA==' is EncryptString('hunter2') (T11's vector, baseunits/uBaseUnit.pas:1559-1573).
    let username =
        String::from_utf8(fmd_lua::crypto::encrypt_string(b"reader@example.test")).unwrap();
    let cookies = String::from_utf8(fmd_lua::crypto::encrypt_string(b"session=xyz")).unwrap();
    json!([
        {
            "ID": MANGADEX,
            "Settings": {
                "Enabled": true,
                "MaxTaskLimit": 2,
                "MaxThreadPerTaskLimit": 3,
                "MaxConnectionLimit": 4,
                "UpdateListNumberOfThread": 0,
                "UpdateListDirectoryPageNumber": 0,
                "HTTP": {
                    "Cookies": "a=1",
                    "UserAgent": "FMD2-UA",
                    "Proxy": {
                        "ProxyType": "ptSOCKS5",
                        "ProxyHost": "proxy.test",
                        "ProxyPort": "1080",
                        "ProxyUsername": "pu",
                        "ProxyPassword": "pp"
                    }
                },
                "OverrideSettings": { "SaveToPath": "D:\\Dex" }
            },
            "Options": { "showscangroup": true, "lang": 2, "token": "abc" },
            "Account": {
                "Enabled": true,
                "Username": username,
                "Password": "pcjhSQpguA==",
                "Status": "asValid",
                "Cookies": cookies
            },
            "Cookies": [
                {
                    "Name": "sid",
                    "Value": "v1",
                    "Domain": "mangadex.org",
                    "Path": "/",
                    "SameSite": "Lax",
                    "Expires": "2030-01-02 03:04:05",
                    "HostOnly": false,
                    "HttpOnly": true,
                    "Secure": true,
                    "Persistent": true
                }
            ]
        },
        {
            "ID": PLAIN,
            "Settings": {
                "Enabled": false,
                "MaxTaskLimit": 0,
                "MaxThreadPerTaskLimit": 0,
                "MaxConnectionLimit": 0,
                "UpdateListNumberOfThread": 0,
                "UpdateListDirectoryPageNumber": 0,
                "HTTP": {
                    "Cookies": "",
                    "UserAgent": "",
                    "Proxy": { "ProxyType": "ptDefault", "ProxyHost": "", "ProxyPort": "", "ProxyUsername": "", "ProxyPassword": "" }
                },
                "OverrideSettings": { "SaveToPath": "" }
            },
            "Cookies": []
        }
    ])
    .to_string()
}

#[test]
fn module_settings_options_and_cookies_are_imported() {
    let fmd2 = Fmd2::new();
    fmd2.file("modules.json", &modules_json());
    let app = App::new();

    let report = app.import(&fmd2);

    assert!(report.module_settings.found);
    // The module with nothing but defaults has nothing to import.
    assert_eq!(report.module_settings.imported, 1);
    let repo = app.db.module_settings();
    assert!(repo.get(PLAIN).unwrap().is_none());

    let o = ModuleOverrides::load(&repo, MANGADEX).unwrap();
    assert!(o.enabled);
    assert_eq!(
        (
            o.limits.max_task_limit,
            o.limits.max_thread_per_task_limit,
            o.limits.max_connection_limit
        ),
        (2, 3, 4)
    );
    assert_eq!(o.http.user_agent, "FMD2-UA");
    assert_eq!(o.http.cookies, "a=1");
    assert_eq!(o.http.proxy.kind, ProxyOverrideType::Socks5);
    assert_eq!(o.http.proxy.host, "proxy.test");
    assert_eq!(o.http.proxy.port, "1080");
    assert_eq!(o.http.proxy.username, "pu");
    assert_eq!(o.http.proxy.password, "pp");
    assert_eq!(o.options["showscangroup"], json!(true));
    assert_eq!(o.options["lang"], json!(2));
    assert_eq!(o.options["token"], json!("abc"));

    let jar = repo.cookie_jar(MANGADEX).unwrap().unwrap();
    let cookies: Vec<fmd_http::Cookie> = serde_json::from_slice(&jar).unwrap();
    assert_eq!(cookies.len(), 1);
    let c = &cookies[0];
    assert_eq!(
        (
            c.name.as_str(),
            c.value.as_str(),
            c.domain.as_str(),
            c.path.as_str()
        ),
        ("sid", "v1", "mangadex.org", "/")
    );
    assert_eq!(c.same_site, "Lax");
    // 2030-01-02 03:04:05 UTC.
    assert_eq!(c.expires, Some(1_893_553_445));
    assert!(c.http_only && c.secure && !c.host_only);

    // The per-module save path has no FMD2r counterpart.
    assert!(report.unmapped.iter().any(|u| u.source == "modules.json"
        && u.key == format!("{MANGADEX} Settings.OverrideSettings.SaveToPath")
        && u.value == "D:\\Dex"));
}

#[test]
fn an_account_is_decrypted_from_fmd2_and_stored_with_fmd2rs_cipher() {
    let fmd2 = Fmd2::new();
    fmd2.file("modules.json", &modules_json());
    let app = App::new();

    let report = app.import(&fmd2);

    assert_eq!(report.accounts.imported, 1);
    let account = app.db.accounts(&app.cipher).get(MANGADEX).unwrap().unwrap();
    assert_eq!(account.password, "hunter2");
    assert_eq!(account.username, "reader@example.test");
    assert_eq!(account.cookies, "session=xyz");
    assert!(account.enabled);
    assert_eq!(account.status, AccountStatus::Valid);
    assert!(app.db.accounts(&app.cipher).get(PLAIN).unwrap().is_none());

    let conn = rusqlite::Connection::open(app.path().join("app.db")).unwrap();
    let stored: Vec<u8> = conn
        .query_row(
            "SELECT password FROM accounts WHERE module_id = ?1",
            [MANGADEX],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(stored, b"hunter2");
    assert_ne!(stored, b"pcjhSQpguA==");
    assert_eq!(app.cipher.decrypt(&stored).unwrap(), b"hunter2");
}

#[test]
fn an_account_left_checking_is_imported_as_unknown() {
    let fmd2 = Fmd2::new();
    // FMD2 resets asChecking to asUnknown when it loads (baseunits/WebsiteModules.pas:612-613).
    fmd2.file(
        "modules.json",
        &json!([{
            "ID": MANGADEX,
            "Account": { "Enabled": false, "Username": "", "Password": "pcjhSQpguA==", "Status": "asChecking", "Cookies": "" }
        }])
        .to_string(),
    );
    let app = App::new();

    app.import(&fmd2);

    let account = app.db.accounts(&app.cipher).get(MANGADEX).unwrap().unwrap();
    assert_eq!(account.status, AccountStatus::Unknown);
    assert_eq!(account.username, "");
    assert_eq!(account.password, "hunter2");
}
