//! The password setting through `PATCH /api/settings`
//! (docs/tickets/T64-password-and-bind-settings.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_server::{AppState, build_router};
use fmd_store::AppDb;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct Harness {
    dir: TempDir,
    db: AppDb,
    state: AppState,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    Harness {
        state: AppState::new(db.clone()).unwrap(),
        db,
        dir,
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

/// `GET /api/inbox`, a protected route.
async fn inbox(state: &AppState, header: Option<(&str, &str)>) -> StatusCode {
    let mut req = Request::get("/api/inbox");
    if let Some((name, value)) = header {
        req = req.header(name, value);
    }
    send(state, req.body(Body::empty()).unwrap()).await.status()
}

async fn inbox_with_bearer(state: &AppState, token: &str) -> StatusCode {
    inbox(state, Some(("authorization", &format!("Bearer {token}")))).await
}

async fn set_password(state: &AppState, password: &str, auth: Option<(&str, &str)>) -> StatusCode {
    let mut req = Request::patch("/api/settings").header("content-type", "application/json");
    if let Some((name, value)) = auth {
        req = req.header(name, value);
    }
    let body = json!({ "server": { "auth_token": password } }).to_string();
    send(state, req.body(Body::from(body)).unwrap())
        .await
        .status()
}

/// Logs in with `password`; the `name=value` cookie pair, or `None` when refused.
async fn login(state: &AppState, password: &str) -> Option<String> {
    let req = Request::post("/api/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "password": password }).to_string()))
        .unwrap();
    let res = send(state, req).await;
    if res.status() != StatusCode::NO_CONTENT {
        return None;
    }
    let cookie = res.headers().get("set-cookie")?.to_str().unwrap();
    Some(cookie.split(';').next().unwrap().to_string())
}

#[tokio::test]
async fn a_password_set_in_the_settings_is_required_by_the_next_request() {
    let h = harness();
    assert_eq!(inbox(&h.state, None).await, StatusCode::OK);

    assert_eq!(
        set_password(&h.state, "hunter2", None).await,
        StatusCode::OK
    );

    assert_eq!(inbox(&h.state, None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        inbox_with_bearer(&h.state, "wrong").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(inbox_with_bearer(&h.state, "hunter2").await, StatusCode::OK);
    assert_eq!(login(&h.state, "wrong").await, None);
    let cookie = login(&h.state, "hunter2").await.unwrap();
    assert_eq!(
        inbox(&h.state, Some(("cookie", &cookie))).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_command_line_password_wins_over_the_setting() {
    let h = harness();
    let bearer = "Bearer hunter2";
    set_password(&h.state, "hunter2", None).await;
    let state = AppState::new(h.db.clone()).unwrap().with_auth("from-env");

    assert_eq!(
        inbox_with_bearer(&state, "hunter2").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(login(&state, "hunter2").await, None);
    assert_eq!(inbox_with_bearer(&state, "from-env").await, StatusCode::OK);

    // Changing the setting changes nothing while the command line sets the password.
    let env = Some(("authorization", "Bearer from-env"));
    assert_eq!(set_password(&state, "", env).await, StatusCode::OK);
    assert_eq!(inbox(&state, None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        inbox(&state, Some(("authorization", bearer))).await,
        StatusCode::UNAUTHORIZED
    );
}

/// The `server` settings group as `app.db` stores it.
fn stored_server(db: &AppDb) -> Value {
    db.settings().get("server").unwrap().unwrap_or_default()
}

/// Asserts that `app.db` stores a salted hash in place of `password`.
fn assert_stored_as_hash(db: &AppDb, password: &str) {
    let stored = stored_server(db);
    let text = stored.to_string();
    assert!(!text.contains(password), "{password} is stored: {text}");
    let token = stored["auth_token"].as_str().unwrap_or_default();
    assert!(
        token.starts_with("$argon2id$"),
        "not an Argon2 hash: {text}"
    );
}

#[tokio::test]
async fn only_a_hash_of_the_password_is_stored() {
    let h = harness();
    set_password(&h.state, "hunter2", None).await;
    assert_stored_as_hash(&h.db, "hunter2");

    // The same password set again gets another salt.
    let first = stored_server(&h.db);
    let bearer = Some(("authorization", "Bearer hunter2"));
    assert_eq!(
        set_password(&h.state, "hunter2", bearer).await,
        StatusCode::OK
    );
    assert_ne!(stored_server(&h.db), first);

    // A restart reads it back.
    let restarted = AppState::new(AppDb::open(h.dir.path().join("app.db")).unwrap()).unwrap();
    assert_eq!(inbox(&restarted, None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        inbox_with_bearer(&restarted, "hunter2").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_password_an_older_build_stored_is_hashed_on_start() {
    let h = harness();
    // As stored before secrets were encrypted (T63).
    h.db.settings()
        .set("server", &json!({ "auth_token": "hunter2" }))
        .unwrap();

    let state = AppState::new(h.db.clone()).unwrap();

    assert_stored_as_hash(&h.db, "hunter2");
    assert_eq!(inbox(&state, None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(inbox_with_bearer(&state, "hunter2").await, StatusCode::OK);
}

/// Whether the response body ends (rather than staying open) within a second.
async fn body_ends(res: Response) -> bool {
    let mut body = res.into_body();
    let drained = async {
        while let Some(frame) = http_body_util::BodyExt::frame(&mut body).await {
            frame.unwrap();
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), drained)
        .await
        .is_ok()
}

#[tokio::test]
async fn changing_the_password_ends_the_sessions() {
    let h = harness();
    set_password(&h.state, "hunter2", None).await;
    let cookie = login(&h.state, "hunter2").await.unwrap();
    let session = Some(("cookie", cookie.as_str()));
    let events = Request::get("/api/events")
        .header("cookie", &cookie)
        .body(Body::empty())
        .unwrap();
    let stream = send(&h.state, events).await;
    assert_eq!(stream.status(), StatusCode::OK);

    assert_eq!(
        set_password(&h.state, "correct horse", session).await,
        StatusCode::OK
    );

    assert_eq!(inbox(&h.state, session).await, StatusCode::UNAUTHORIZED);
    assert!(body_ends(stream).await, "the event stream stayed open");
    assert_eq!(
        inbox_with_bearer(&h.state, "correct horse").await,
        StatusCode::OK
    );
}

async fn health(state: &AppState) -> Value {
    let res = send(
        state,
        Request::get("/api/health").body(Body::empty()).unwrap(),
    )
    .await;
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_says_whether_an_open_server_is_reachable_from_other_machines() {
    let h = harness();
    let any: std::net::SocketAddr = "0.0.0.0:8080".parse().unwrap();
    let exposed = h.state.clone().with_listen_addr(any);
    let body = health(&exposed).await;
    assert_eq!(body["auth"], false);
    assert_eq!(body["loopback"], false);

    let local = h
        .state
        .clone()
        .with_listen_addr("127.0.0.1:8080".parse().unwrap());
    assert_eq!(health(&local).await["loopback"], true);

    set_password(&exposed, "hunter2", None).await;
    assert_eq!(health(&exposed).await["auth"], true);
}

#[tokio::test]
async fn health_names_the_settings_the_command_line_overrides() {
    let h = harness();
    assert_eq!(health(&h.state).await["overridden"], json!([]));

    let state = h
        .state
        .clone()
        .with_auth("from-env")
        .with_overridden(["server.bind"]);
    let body = health(&state).await;
    let mut overridden: Vec<&str> = body["overridden"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    overridden.sort_unstable();
    assert_eq!(overridden, ["server.auth_token", "server.bind"]);
}

#[tokio::test]
async fn a_stored_password_that_cannot_be_decrypted_keeps_the_server_locked() {
    let h = harness();
    // Encrypted by an older build (T63) under a key file that is gone.
    h.db.settings()
        .set("server", &json!({ "auth_token": { "encrypted": "00ff" } }))
        .unwrap();

    let state = AppState::new(h.db.clone()).unwrap();

    assert_eq!(health(&state).await["auth"], true);
    assert_eq!(inbox(&state, None).await, StatusCode::UNAUTHORIZED);
    // The command line password still gets in, to set a new one.
    let state = state.with_auth("from-env");
    assert_eq!(inbox_with_bearer(&state, "from-env").await, StatusCode::OK);
}
