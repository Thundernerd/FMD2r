//! Settings secrets are never stored in plain text in `app.db`, and an older build's plain
//! values are encrypted on the next start (docs/tickets/T63-settings-secrets.md).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use fmd_core::settings::{
    ModuleOverrides, ModulePatch, SettingsService, StoredModuleHttpSettings, verify_password,
};
use fmd_http::{Proxy, ProxyKind};
use fmd_lua::{ModuleHttpSettings, ProxyOverride};
use fmd_store::{AppDb, ModuleSettings};
use serde_json::{Value, json};

const SECRETS: [&str; 4] = [
    "proxy-secret",
    "token-secret",
    "server-secret",
    "module-secret",
];

fn db() -> (AppDb, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (AppDb::open(dir.path().join("app.db")).unwrap(), dir)
}

/// Everything `app.db` stores for the settings and the module `site`, as text.
fn stored_text(db: &AppDb) -> String {
    let mut text = String::new();
    for key in ["connections", "module_updater", "server"] {
        let group: Option<Value> = db.settings().get(key).unwrap();
        text += &group.unwrap_or_default().to_string();
    }
    if let Some(module) = db.module_settings().get("site").unwrap() {
        text += &module.http.to_string();
    }
    text
}

fn assert_no_plain_secret(db: &AppDb) {
    let text = stored_text(db);
    for secret in SECRETS {
        assert!(
            !text.contains(secret),
            "{secret} is stored in plain text: {text}"
        );
    }
}

/// The proxy the module `site`'s requests go through.
fn module_proxy(db: &AppDb) -> ProxyOverride {
    StoredModuleHttpSettings::new(db.clone(), "site")
        .http_overrides()
        .unwrap()
        .proxy
}

fn expected_module_proxy() -> ProxyOverride {
    ProxyOverride::Proxy(Proxy {
        kind: ProxyKind::Http,
        host: "proxy.example".into(),
        port: "8080".into(),
        user: "me".into(),
        pass: "module-secret".into(),
    })
}

fn assert_secrets_readable(db: &AppDb) {
    let s = SettingsService::load(db.clone()).unwrap().get();
    assert_eq!(s.connections.proxy.password, "proxy-secret");
    assert_eq!(
        s.module_updater.github_token.as_deref(),
        Some("token-secret")
    );
    let hash = s.server.auth_token.as_deref().unwrap_or_default();
    assert!(verify_password(hash, "server-secret"));
    let overrides = ModuleOverrides::load(&db.module_settings(), "site").unwrap();
    assert_eq!(overrides.http.proxy.password, "module-secret");
    assert_eq!(module_proxy(db), expected_module_proxy());
}

const MODULE_HTTP: &str = r#"{ "proxy": { "type": "http", "host": "proxy.example",
    "port": "8080", "username": "me", "password": "module-secret" } }"#;

#[test]
fn secrets_are_stored_encrypted_and_read_back() {
    let (db, _dir) = db();
    let service = SettingsService::load(db.clone()).unwrap();
    service
        .update(json!({
            "connections": { "proxy": { "password": "proxy-secret" } },
            "module_updater": { "github_token": "token-secret" },
        }))
        .unwrap();
    let module_http: Value = serde_json::from_str(MODULE_HTTP).unwrap();
    service
        .update_with_modules(
            json!({ "server": { "auth_token": "server-secret" } }),
            vec![ModulePatch {
                module_id: "site".into(),
                options: Vec::new(),
                patch: json!({ "enabled": true, "http": module_http }),
            }],
        )
        .unwrap();

    assert_no_plain_secret(&db);
    assert_secrets_readable(&db);
}

#[test]
fn a_module_proxy_password_saved_directly_is_stored_encrypted() {
    let (db, _dir) = db();
    let repo = db.module_settings();
    let mut overrides = ModuleOverrides::default();
    overrides
        .apply_patch(
            &[],
            json!({ "enabled": true, "http": serde_json::from_str::<Value>(MODULE_HTTP).unwrap() }),
        )
        .unwrap();
    overrides.save(&repo, "site").unwrap();

    assert_no_plain_secret(&db);
    assert_eq!(module_proxy(&db), expected_module_proxy());
}

#[test]
fn plain_secrets_from_an_older_build_are_encrypted_on_start() {
    let (db, _dir) = db();
    let settings = db.settings();
    settings
        .set(
            "connections",
            &json!({ "proxy": { "enabled": true, "password": "proxy-secret" } }),
        )
        .unwrap();
    settings
        .set("module_updater", &json!({ "github_token": "token-secret" }))
        .unwrap();
    settings
        .set("server", &json!({ "auth_token": "server-secret" }))
        .unwrap();
    let mut module = ModuleSettings::new("site");
    module.enabled = true;
    module.http = serde_json::from_str(MODULE_HTTP).unwrap();
    module.cookie_jar = Some(b"jar".to_vec());
    db.module_settings().upsert(&module).unwrap();

    let service = SettingsService::load(db.clone()).unwrap();
    assert_eq!(service.get().connections.proxy.password, "proxy-secret");
    assert!(service.get().connections.proxy.enabled);

    assert_no_plain_secret(&db);
    assert_secrets_readable(&db);
    assert_eq!(
        db.module_settings().cookie_jar("site").unwrap().as_deref(),
        Some(&b"jar"[..])
    );
}

/// An empty token or server password is no token or password: clearing one unsets it.
#[test]
fn an_empty_token_or_server_password_unsets_it() {
    let (db, _dir) = db();
    let service = SettingsService::load(db).unwrap();
    service
        .update(json!({
            "module_updater": { "github_token": "token-secret" },
            "server": { "auth_token": "server-secret" },
        }))
        .unwrap();
    let s = service
        .update(json!({
            "module_updater": { "github_token": "" },
            "server": { "auth_token": "" },
        }))
        .unwrap();
    assert_eq!(s.module_updater.github_token, None);
    assert_eq!(s.server.auth_token, None);
}
