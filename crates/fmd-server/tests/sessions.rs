//! Session cookies: expiry, sliding renewal, logout and revocation, driven through `build_router`
//! with `oneshot` and an injected clock.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_server::{AppState, build_router};
use fmd_store::AppDb;
use tempfile::TempDir;
use tower::ServiceExt;

const SECRET: &str = "hunter2";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A clock the test moves forward by hand.
#[derive(Clone)]
struct TestClock(Arc<AtomicU64>);

impl TestClock {
    fn new() -> Self {
        // 2026-01-01T00:00:00Z
        Self(Arc::new(AtomicU64::new(1_767_225_600)))
    }

    fn advance(&self, by: Duration) {
        self.0.fetch_add(by.as_secs(), Ordering::SeqCst);
    }

    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

struct Harness {
    dir: TempDir,
    clock: TestClock,
    state: AppState,
}

fn state(db: AppDb, secret: &str, clock: &TestClock) -> AppState {
    let clock = clock.clone();
    AppState::new(db)
        .unwrap()
        .with_auth(secret)
        .with_clock(move || clock.now())
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let clock = TestClock::new();
    Harness {
        state: state(db, SECRET, &clock),
        dir,
        clock,
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

/// Logs in and returns the `name=value` cookie pair.
async fn login(state: &AppState) -> String {
    let req = Request::post("/api/login")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "password": SECRET }).to_string(),
        ))
        .unwrap();
    let res = send(state, req).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let cookie = res.headers()["set-cookie"].to_str().unwrap();
    cookie.split(';').next().unwrap().to_string()
}

async fn inbox_with(state: &AppState, cookie: &str) -> StatusCode {
    let req = Request::get("/api/inbox")
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap();
    send(state, req).await.status()
}

#[tokio::test]
async fn a_session_expires_after_the_idle_timeout() {
    let h = harness();
    let cookie = login(&h.state).await;
    assert_eq!(inbox_with(&h.state, &cookie).await, StatusCode::OK);

    h.clock.advance(7 * DAY + Duration::from_secs(1));
    assert_eq!(
        inbox_with(&h.state, &cookie).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn using_a_session_renews_it_until_its_absolute_lifetime() {
    let h = harness();
    let cookie = login(&h.state).await;
    // Every six days for 29 days: never idle for 7, so the session stays live.
    for _ in 0..4 {
        h.clock.advance(6 * DAY);
        assert_eq!(inbox_with(&h.state, &cookie).await, StatusCode::OK);
    }
    h.clock.advance(5 * DAY);
    assert_eq!(inbox_with(&h.state, &cookie).await, StatusCode::OK);

    // Past 30 days since login it ends, used or not.
    h.clock.advance(DAY + Duration::from_secs(1));
    assert_eq!(
        inbox_with(&h.state, &cookie).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_timeouts_follow_the_server_settings() {
    let h = harness();
    let patch = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {SECRET}"))
        .body(Body::from(
            serde_json::json!({ "server": { "session_idle_days": 1 } }).to_string(),
        ))
        .unwrap();
    assert_eq!(send(&h.state, patch).await.status(), StatusCode::OK);
    let cookie = login(&h.state).await;

    h.clock.advance(DAY + Duration::from_secs(1));
    assert_eq!(
        inbox_with(&h.state, &cookie).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_cookie_is_secure_only_behind_an_https_proxy() {
    let h = harness();
    let login_via = |proxy: Option<(&str, &str)>| {
        let mut req = Request::post("/api/login").header("content-type", "application/json");
        if let Some((name, value)) = proxy {
            req = req.header(name, value);
        }
        req.body(Body::from(
            serde_json::json!({ "password": SECRET }).to_string(),
        ))
        .unwrap()
    };
    let set_cookie = |res: &Response| res.headers()["set-cookie"].to_str().unwrap().to_owned();

    let plain = set_cookie(&send(&h.state, login_via(None)).await);
    assert!(!plain.contains("Secure"), "{plain}");
    assert!(plain.contains("HttpOnly"), "{plain}");
    assert!(plain.contains("Max-Age=2592000"), "{plain}");

    let forwarded = login_via(Some(("x-forwarded-proto", "https")));
    let forwarded = set_cookie(&send(&h.state, forwarded).await);
    assert!(forwarded.contains("; Secure"), "{forwarded}");

    let standard = login_via(Some(("forwarded", "for=10.0.0.1;proto=https")));
    let standard = set_cookie(&send(&h.state, standard).await);
    assert!(standard.contains("; Secure"), "{standard}");
}

fn post_with(uri: &str, cookie: &str) -> Request<Body> {
    Request::post(uri)
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn logout_ends_the_current_session_and_clears_the_cookie() {
    let h = harness();
    let cookie = login(&h.state).await;
    let other = login(&h.state).await;

    let res = send(&h.state, post_with("/api/logout", &cookie)).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let cleared = res.headers()["set-cookie"].to_str().unwrap();
    assert!(cleared.starts_with("fmd2r_session=;"), "{cleared}");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");

    assert_eq!(
        inbox_with(&h.state, &cookie).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(inbox_with(&h.state, &other).await, StatusCode::OK);
}

#[tokio::test]
async fn revoke_all_ends_every_session() {
    let h = harness();
    let first = login(&h.state).await;
    let second = login(&h.state).await;

    let res = send(&h.state, post_with("/api/sessions/revoke-all", &first)).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    for cookie in [&first, &second] {
        assert_eq!(inbox_with(&h.state, cookie).await, StatusCode::UNAUTHORIZED);
    }
    // The bearer token is the password itself, so it keeps working.
    let bearer = Request::get("/api/inbox")
        .header("authorization", format!("Bearer {SECRET}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&h.state, bearer).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn revoke_all_needs_auth() {
    let h = harness();
    let res = send(
        &h.state,
        post_with("/api/sessions/revoke-all", "fmd2r_session=forged"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn changing_the_password_revokes_every_session() {
    let h = harness();
    let cookie = login(&h.state).await;
    drop(h.state);

    // Restarted with another password over the same app.db.
    let db = AppDb::open(h.dir.path().join("app.db")).unwrap();
    let restarted = state(db, "correct horse", &h.clock);
    assert_eq!(
        inbox_with(&restarted, &cookie).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn health_says_whether_a_password_is_required() {
    let h = harness();
    let get = || Request::get("/api/health").body(Body::empty()).unwrap();
    let body = |res: Response| async {
        let bytes = http_body_util::BodyExt::collect(res.into_body())
            .await
            .unwrap()
            .to_bytes();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };
    assert_eq!(body(send(&h.state, get()).await).await["auth"], true);

    let open = AppState::new(AppDb::open(h.dir.path().join("open.db")).unwrap()).unwrap();
    assert_eq!(body(send(&open, get()).await).await["auth"], false);
}

/// Whether the response body ends (rather than staying open) within a second.
async fn body_ends(res: Response) -> bool {
    let mut body = res.into_body();
    let drained = async {
        while let Some(frame) = http_body_util::BodyExt::frame(&mut body).await {
            frame.unwrap();
        }
    };
    tokio::time::timeout(Duration::from_secs(1), drained)
        .await
        .is_ok()
}

#[tokio::test]
async fn ending_sessions_closes_the_open_event_streams() {
    for end in ["/api/logout", "/api/sessions/revoke-all"] {
        let h = harness();
        let cookie = login(&h.state).await;
        let events = Request::get("/api/events")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        let stream = send(&h.state, events).await;
        assert_eq!(stream.status(), StatusCode::OK);

        let res = send(&h.state, post_with(end, &cookie)).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(body_ends(stream).await, "{end} left the stream open");
    }
}
