//! What the anti-bot hook persists: a module's HTTP settings in `app.db`
//! (docs/tickets/T30-anti-bot.md), and the `websitebypass_config.json` written from the
//! settings.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_core::settings::{
    HttpOverrides, ModuleOverrides, ProxyOverride, ProxyOverrideType, StoredModuleHttpSettings,
};
use fmd_lua::{ModuleHttpOverrides, ModuleHttpSettings};
use fmd_store::AppDb;

fn db() -> (AppDb, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (AppDb::open(dir.path().join("app.db")).unwrap(), dir)
}

#[test]
fn a_bypass_enables_the_module_settings_and_stores_cookies_and_user_agent() {
    let (db, _dir) = db();
    let proxy = ProxyOverride {
        kind: ProxyOverrideType::Direct,
        ..ProxyOverride::default()
    };
    let stored = ModuleOverrides {
        http: HttpOverrides {
            cookies: "old=1".into(),
            proxy: proxy.clone(),
            ..HttpOverrides::default()
        },
        ..ModuleOverrides::default()
    };
    stored.save(&db.module_settings(), "site").unwrap();
    let settings = StoredModuleHttpSettings::new(db.clone(), "site");
    assert_eq!(settings.http_overrides(), None);

    settings.clear_cookies().unwrap();
    assert_eq!(
        ModuleOverrides::load(&db.module_settings(), "site")
            .unwrap()
            .http
            .cookies,
        ""
    );

    settings
        .store_bypass("cf_clearance=solved;", "Solver/1.0")
        .unwrap();

    // baseunits/lua/LuaWebsiteBypass.pas:182-184
    let loaded = ModuleOverrides::load(&db.module_settings(), "site").unwrap();
    assert!(loaded.enabled);
    assert_eq!(loaded.http.cookies, "cf_clearance=solved;");
    assert_eq!(loaded.http.user_agent, "Solver/1.0");
    assert_eq!(loaded.http.proxy, proxy);
    assert_eq!(
        settings.http_overrides(),
        Some(ModuleHttpOverrides {
            user_agent: "Solver/1.0".into(),
            cookies: "cf_clearance=solved;".into(),
            proxy: fmd_lua::ProxyOverride::Direct,
        })
    );
}

fn written_config(existing: Option<&str>, flaresolverr_url: &str) -> serde_json::Value {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("websitebypass/websitebypass_config.json");
    if let Some(existing) = existing {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, existing).unwrap();
    }
    fmd_core::settings::write_websitebypass_config(dir.path(), flaresolverr_url).unwrap();
    serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap()
}

#[test]
fn a_flaresolverr_url_turns_the_webdriver_path_on_and_points_it_there() {
    // The keys cloudflare.lua reads (lua/websitebypass/cloudflare.lua:309-322).
    assert_eq!(
        written_config(None, "http://flaresolverr:8191"),
        serde_json::json!({
            "use_webdriver": true,
            "debug": false,
            "testing": false,
            "flaresolverr_ip": "flaresolverr",
            "flaresolverr_port": 8191
        })
    );
    assert_eq!(
        written_config(None, "http://10.0.0.5:9000/")["flaresolverr_port"],
        9000
    );
}

#[test]
fn without_a_flaresolverr_url_the_config_is_upstreams_default() {
    // lua/websitebypass/websitebypass_config.json
    assert_eq!(
        written_config(None, ""),
        serde_json::json!({
            "use_webdriver": false,
            "debug": false,
            "testing": false,
            "flaresolverr_ip": "localhost",
            "flaresolverr_port": 8191
        })
    );
}

#[test]
fn the_users_debug_flags_survive_a_rewrite() {
    let config = written_config(
        Some(r#"{"use_webdriver": false, "debug": true, "testing": true, "flaresolverr_ip": "x"}"#),
        "http://localhost:8191",
    );
    assert_eq!(config["debug"], true);
    assert_eq!(config["testing"], true);
    assert_eq!(config["use_webdriver"], true);
    assert_eq!(config["flaresolverr_ip"], "localhost");
}
