//! Per-module settings endpoints (`/api/modules`, `/api/modules/{id}/settings`) driven through
//! `build_router` with `oneshot`, over a fixture Lua module declaring one option of each
//! `AddOption*` kind.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::modules::{ModuleInfo, StoreModuleSettings};
use fmd_lua::ModuleRegistry;
use fmd_server::{AppState, ModuleCatalog, ModulesReport, build_router};
use fmd_store::AppDb;
use http_body_util::BodyExt;
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;

const FIXTURE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'fixture'
  m.Name = 'Fixture'
  m.RootURL = 'https://fixture.example'
  m.MaxTaskLimit = 2
  m.MaxConnectionLimit = 4
  m.AddOptionCheckBox('hq', 'High quality', true)
  m.AddOptionEdit('lang', 'Language', 'en')
  m.AddOptionSpinEdit('delay', 'Delay', 3)
  m.AddOptionComboBox('server', 'Server', 'One' .. "\r\n" .. 'Two', 1)
end
"#;

/// The loaded fixture registry served as the module catalog.
struct Registry(Arc<ModuleRegistry>);

impl ModuleCatalog for Registry {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        self.0
            .modules()
            .iter()
            .map(|m| ModuleInfo::from(&m.def()))
            .collect()
    }
}

struct Harness {
    _dir: TempDir,
    state: AppState,
    registry: Arc<ModuleRegistry>,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let lua = dir.path().join("lua");
    std::fs::create_dir_all(lua.join("modules")).unwrap();
    std::fs::write(lua.join("modules/Fixture.lua"), FIXTURE).unwrap();
    let report =
        ModuleRegistry::load_dir_with(&lua, Arc::new(StoreModuleSettings::new(db.clone())));
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let registry = Arc::new(report.registry);
    let state = AppState::new(db)
        .unwrap()
        .with_modules(Registry(registry.clone()));
    Harness {
        _dir: dir,
        state,
        registry,
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn body_json(res: Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn module_settings_list_every_option_kind_with_its_default() {
    let h = harness();
    let res = send(&h.state, get("/api/modules/fixture/settings")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;

    // TLuaWebsiteModule.AddOption* (baseunits/lua/LuaWebsiteModules.pas:768-818); a spin edit
    // spans 0..10000 (mangadownloader/forms/frmWebsiteOptionCustom.pas:165-166) and combo items
    // are lines (`Items.Text`, :410).
    assert_eq!(
        body["options"],
        json!([
            { "kind": "checkbox", "key": "hq", "caption": "High quality",
              "default": true, "value": true },
            { "kind": "edit", "key": "lang", "caption": "Language",
              "default": "en", "value": "en" },
            { "kind": "spinedit", "key": "delay", "caption": "Delay",
              "default": 3, "value": 3, "min": 0, "max": 10000 },
            { "kind": "combobox", "key": "server", "caption": "Server",
              "items": ["One", "Two"], "default": 1, "value": 1 },
        ])
    );
    assert_eq!(body["id"], "fixture");
    assert_eq!(body["name"], "Fixture");
    assert_eq!(body["enabled"], false);
    assert_eq!(
        body["module_limits"],
        json!({ "max_task_limit": 2, "max_thread_per_task_limit": 0, "max_connection_limit": 4 })
    );
    assert_eq!(
        body["limits"],
        json!({ "max_task_limit": 0, "max_thread_per_task_limit": 0, "max_connection_limit": 0 })
    );
    assert_eq!(body["http"]["user_agent"], "");
    assert_eq!(body["http"]["proxy"]["type"], "default");
}

#[tokio::test]
async fn settings_of_an_unknown_module_are_a_404() {
    let h = harness();
    let res = send(&h.state, get("/api/modules/nope/settings")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

fn patch_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::patch(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn a_combo_index_out_of_range_is_a_422_naming_the_option() {
    let h = harness();
    // The combo box is a drop-down list of its items
    // (mangadownloader/forms/frmWebsiteOptionCustom.pas:187-191): "One", "Two" → 0..=1.
    for bad in [json!(2), json!(-1), json!("1")] {
        let patch = json!({ "options": { "server": bad } });
        let res = send(&h.state, patch_json("/api/modules/fixture/settings", patch)).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(body_json(res).await["field"], "options.server");
    }

    let body = body_json(send(&h.state, get("/api/modules/fixture/settings")).await).await;
    assert_eq!(body["options"][3]["value"], 1);
}

#[tokio::test]
async fn a_valid_patch_persists_and_get_option_sees_it() {
    let h = harness();
    let patch = json!({
        "enabled": true,
        "limits": { "max_task_limit": 1 },
        "http": { "user_agent": "Custom/1.0", "proxy": { "type": "direct" } },
        "options": { "hq": false, "lang": "de", "delay": 10000, "server": 0 },
    });
    let res = send(&h.state, patch_json("/api/modules/fixture/settings", patch)).await;
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_json(send(&h.state, get("/api/modules/fixture/settings")).await).await;
    let values: Vec<_> = body["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["value"].clone())
        .collect();
    assert_eq!(values, [json!(false), json!("de"), json!(10000), json!(0)]);
    assert_eq!(body["enabled"], true);
    assert_eq!(body["limits"]["max_task_limit"], 1);
    assert_eq!(body["http"]["user_agent"], "Custom/1.0");
    assert_eq!(body["http"]["proxy"]["type"], "direct");

    let runtime = fmd_lua::Runtime::new().unwrap();
    runtime
        .set_module(h.registry.get("fixture").unwrap())
        .unwrap();
    runtime
        .exec(
            r#"
            assert(MODULE.GetOption('hq') == false)
            assert(MODULE.GetOption('lang') == 'de')
            assert(MODULE.GetOption('delay') == 10000)
            assert(MODULE.GetOption('server') == 0)
            "#,
        )
        .unwrap();

    // `null` resets an option to its declared default.
    let patch = json!({ "options": { "lang": null } });
    let res = send(&h.state, patch_json("/api/modules/fixture/settings", patch)).await;
    assert_eq!(body_json(res).await["options"][1]["value"], "en");
    runtime
        .exec("assert(MODULE.GetOption('lang') == 'en')")
        .unwrap();
}

#[tokio::test]
async fn invalid_overrides_are_422s_naming_the_field() {
    let h = harness();
    for (patch, field) in [
        (json!({ "options": { "hq": 1 } }), "options.hq"),
        (json!({ "options": { "lang": 5 } }), "options.lang"),
        (json!({ "options": { "delay": 10001 } }), "options.delay"),
        (json!({ "options": { "missing": 1 } }), "options.missing"),
        (
            json!({ "limits": { "max_task_limit": -1 } }),
            "limits.max_task_limit",
        ),
        (
            json!({ "http": { "proxy": { "type": "ftp" } } }),
            "http.proxy.type",
        ),
        (json!({ "colour": "red" }), "colour"),
    ] {
        let res = send(&h.state, patch_json("/api/modules/fixture/settings", patch)).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{field}");
        assert_eq!(body_json(res).await["field"], field);
    }
}

#[tokio::test]
async fn modules_are_listed_for_the_picker() {
    let h = harness();
    let body = body_json(send(&h.state, get("/api/modules")).await).await;
    assert_eq!(
        body,
        json!([{
            "id": "fixture",
            "name": "Fixture",
            "category": "",
            "option_count": 4,
            "capabilities": { "update_list": false, "info": false, "download": false, "account": false },
            "list_size": 0,
            "list_updated": null,
            "list_job_running": false,
        }])
    );
}
