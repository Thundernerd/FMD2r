//! Account endpoints (`/api/accounts`) driven through `build_router` with `oneshot`, over the
//! ticket's fixture module (docs/tickets/T31-accounts-login.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::accounts::AccountService;
use fmd_core::modules::StoreModuleSettings;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_server::{AppState, build_router};
use fmd_store::{AppDb, KeyFileCipher};
use http_body_util::BodyExt;
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;

const FIXTURE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'fixture'
  m.Name = 'Fixture'
  m.AccountSupport = true
  m.OnLogin = 'Login'
end

function Login()
  if MODULE.Account.Username == 'u' and MODULE.Account.Password == 'p' then
    MODULE.Account.Cookies = 'sid=1'; MODULE.Account.Status = asValid; return true
  end
  MODULE.Account.Status = asInvalid; return false
end
"#;

/// A module without account support, which the account list leaves out.
const PLAIN: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'plain'
  m.Name = 'Plain'
end
"#;

/// Never reaches the network: every request fails.
struct Offline;

impl Transport for Offline {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("offline".into())) })
    }
}

fn harness() -> (TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let cipher = Arc::new(KeyFileCipher::open_or_create(dir.path().join("accounts.key")).unwrap());
    let lua = dir.path().join("lua");
    std::fs::create_dir_all(lua.join("modules")).unwrap();
    std::fs::write(lua.join("modules/Fixture.lua"), FIXTURE).unwrap();
    std::fs::write(lua.join("modules/Plain.lua"), PLAIN).unwrap();
    let report =
        ModuleRegistry::load_dir_with(&lua, Arc::new(StoreModuleSettings::new(db.clone(), cipher)));
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let mut config = PoolConfig::new(HttpClient::with_transport(Arc::new(Offline)).unwrap());
    config.threads = 1;
    config.lua_dir = lua;
    let pool = Arc::new(WorkerPool::new(config).unwrap());
    let service = AccountService::new(Arc::new(report.registry), pool);
    let state = AppState::new(db).unwrap().with_accounts(Arc::new(service));
    (dir, state)
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

fn json_request(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn empty(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

async fn body_text(res: Response) -> String {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn put_then_get_never_returns_the_password() {
    let (_dir, state) = harness();

    let res = send(
        &state,
        json_request(
            "PUT",
            "/api/accounts/fixture",
            json!({"username": "u", "password": "secret-pw", "enabled": true}),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!body_text(res).await.contains("secret-pw"));

    let res = send(&state, empty("GET", "/api/accounts")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let text = body_text(res).await;
    assert!(!text.contains("secret-pw"), "{text}");
    let listed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        listed,
        json!([{
            "module": "fixture",
            "name": "Fixture",
            "enabled": true,
            "username": "u",
            "has_password": true,
            "status": "unknown"
        }])
    );
}

#[tokio::test]
async fn login_returns_the_status_the_module_set() {
    let (_dir, state) = harness();
    let put = |password: &str| {
        json_request(
            "PUT",
            "/api/accounts/fixture",
            json!({"username": "u", "password": password, "enabled": true}),
        )
    };

    send(&state, put("p")).await;
    let res = send(&state, empty("POST", "/api/accounts/fixture/login")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_text(res).await).unwrap();
    assert_eq!(body["status"], "valid");

    send(&state, put("wrong")).await;
    let res = send(&state, empty("POST", "/api/accounts/fixture/login")).await;
    let body: serde_json::Value = serde_json::from_str(&body_text(res).await).unwrap();
    assert_eq!(body["status"], "invalid");
}

#[tokio::test]
async fn a_put_without_password_keeps_the_stored_one() {
    let (_dir, state) = harness();
    send(
        &state,
        json_request(
            "PUT",
            "/api/accounts/fixture",
            json!({"username": "u", "password": "p"}),
        ),
    )
    .await;
    send(
        &state,
        json_request("PUT", "/api/accounts/fixture", json!({"enabled": true})),
    )
    .await;

    let res = send(&state, empty("POST", "/api/accounts/fixture/login")).await;
    let body: serde_json::Value = serde_json::from_str(&body_text(res).await).unwrap();
    assert_eq!(body["status"], "valid");
    assert_eq!(body["enabled"], true);
}

#[tokio::test]
async fn delete_clears_the_account() {
    let (_dir, state) = harness();
    send(
        &state,
        json_request(
            "PUT",
            "/api/accounts/fixture",
            json!({"username": "u", "password": "p", "enabled": true}),
        ),
    )
    .await;

    let res = send(&state, empty("DELETE", "/api/accounts/fixture")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = send(&state, empty("GET", "/api/accounts")).await;
    let listed: serde_json::Value = serde_json::from_str(&body_text(res).await).unwrap();
    assert_eq!(listed[0]["username"], "");
    assert_eq!(listed[0]["has_password"], false);
    assert_eq!(listed[0]["enabled"], false);
}

#[tokio::test]
async fn modules_without_account_support_are_not_found() {
    let (_dir, state) = harness();
    for req in [
        json_request("PUT", "/api/accounts/plain", json!({"username": "u"})),
        empty("POST", "/api/accounts/plain/login"),
        empty("POST", "/api/accounts/missing/login"),
        empty("DELETE", "/api/accounts/missing"),
    ] {
        assert_eq!(send(&state, req).await.status(), StatusCode::NOT_FOUND);
    }
}
