//! Optional single-user auth: a configured password/token, sent as `Authorization: Bearer` or
//! traded for a session cookie at `POST /api/login`.
//!
//! Sessions live in `app.db` and end after `server.session_idle_days` without use, after
//! `server.session_lifetime_days` in any case, at `POST /api/logout`, at
//! `POST /api/sessions/revoke-all`, and when the password changes. No FMD2 counterpart: FMD2 has
//! no web server.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::{ApiError, AppState, Problem};

const COOKIE: &str = "fmd2r_session";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The configured secret.
pub(crate) struct Auth {
    secret: String,
}

impl Auth {
    pub(crate) fn new(secret: String) -> Arc<Self> {
        Arc::new(Self { secret })
    }

    fn is_secret(&self, candidate: &str) -> bool {
        ct_eq(candidate, &self.secret)
    }

    /// What `app.db` stores for the session cookie `token`: a hash bound to the secret, so a
    /// password change leaves every stored session unmatched.
    fn token_hash(&self, token: &str) -> Vec<u8> {
        let mut hash = Sha256::new();
        hash.update(b"fmd2r-session\0");
        hash.update((self.secret.len() as u64).to_le_bytes());
        hash.update(self.secret.as_bytes());
        hash.update(token.as_bytes());
        hash.finalize().to_vec()
    }

    fn bearer_authorizes(&self, headers: &HeaderMap) -> bool {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .is_some_and(|(_, token)| self.is_secret(token.trim()))
    }
}

/// Constant-time string comparison (the length itself is not hidden).
fn ct_eq(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// The session cookie values sent with a request.
fn session_cookies(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('='))
        .map(str::to_owned)
        .collect()
}

/// Unix milliseconds of `at`, saturating.
fn unix_ms(at: SystemTime) -> i64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The session timing at the current time: now, and the oldest last use and creation a live
/// session may have.
struct Window {
    now: i64,
    seen_since: i64,
    created_since: i64,
    lifetime: Duration,
}

impl Window {
    fn current(state: &AppState) -> Self {
        let now = unix_ms(state.now());
        let server = &state.settings.get().server;
        let days = |d: u32| i64::from(d).saturating_mul(DAY_MS);
        Self {
            now,
            seen_since: now.saturating_sub(days(server.session_idle_days)),
            created_since: now.saturating_sub(days(server.session_lifetime_days)),
            lifetime: Duration::from_secs(u64::from(server.session_lifetime_days) * 86_400),
        }
    }
}

/// Whether a session cookie in `headers` names a live session; renews the ones that do.
async fn session_authorizes(
    state: &AppState,
    auth: &Auth,
    headers: &HeaderMap,
) -> Result<bool, ApiError> {
    let hashes: Vec<Vec<u8>> = session_cookies(headers)
        .iter()
        .map(|t| auth.token_hash(t))
        .collect();
    if hashes.is_empty() {
        return Ok(false);
    }
    let w = Window::current(state);
    state
        .blocking(move |db| {
            let sessions = db.sessions();
            let mut live = false;
            for hash in &hashes {
                live |= sessions.renew(hash, w.now, w.seen_since, w.created_since)?;
            }
            Ok::<_, fmd_store::StoreError>(live)
        })
        .await
}

/// Middleware for protected routes: a no-op unless auth is configured.
pub(crate) async fn require(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let Some(auth) = state.auth.clone() else {
        return next.run(req).await;
    };
    if auth.bearer_authorizes(req.headers()) {
        return next.run(req).await;
    }
    match session_authorizes(&state, &auth, req.headers()).await {
        Ok(true) => next.run(req).await,
        Ok(false) => ApiError::Unauthorized.into_response(),
        Err(e) => e.into_response(),
    }
}

/// Whether the client reached us over HTTPS, directly or through a proxy that says so
/// (`X-Forwarded-Proto` or `Forwarded: proto=`).
fn over_https(req_headers: &HeaderMap) -> bool {
    let forwarded_proto = req_headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .is_some_and(|p| p.trim().eq_ignore_ascii_case("https"));
    let forwarded = req_headers
        .get(header::FORWARDED)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .is_some_and(|first| {
            first.split(';').any(|kv| {
                kv.trim().split_once('=').is_some_and(|(k, v)| {
                    k.eq_ignore_ascii_case("proto")
                        && v.trim_matches('"').eq_ignore_ascii_case("https")
                })
            })
        });
    forwarded_proto || forwarded
}

/// A `Set-Cookie` header for the session cookie holding `value` for `max_age`.
fn session_cookie(value: &str, max_age: Duration, secure: bool) -> Result<HeaderValue, ApiError> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{COOKIE}={value}; Path=/; Max-Age={}; HttpOnly; SameSite=Strict{secure}",
        max_age.as_secs()
    );
    HeaderValue::from_str(&cookie).map_err(|e| ApiError::Internal(e.to_string()))
}

/// A response that tells the browser to drop the session cookie.
fn clearing_cookie(status: StatusCode, headers: &HeaderMap) -> Result<Response, ApiError> {
    let cookie = session_cookie("", Duration::ZERO, over_https(headers))?;
    Ok((status, [(header::SET_COOKIE, cookie)]).into_response())
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct Login {
    password: String,
}

/// Trade the configured password for a session cookie.
#[utoipa::path(post, path = "/api/login", tag = "auth", operation_id = "login", request_body = Login,
    responses(
        (status = 204, description = "Logged in; the session cookie is set (nothing to do when auth is off)"),
        (status = 401, description = "Wrong password", body = Problem),
    ))]
pub(crate) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(login): ApiJson<Login>,
) -> Result<Response, ApiError> {
    let Some(auth) = state.auth.clone() else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    if !auth.is_secret(&login.password) {
        return Err(ApiError::Unauthorized);
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| ApiError::Internal(e.to_string()))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let hash = auth.token_hash(&token);
    let w = Window::current(&state);
    state
        .blocking(move |db| {
            let sessions = db.sessions();
            sessions.delete_expired(w.seen_since, w.created_since)?;
            sessions.create(&hash, w.now)
        })
        .await?;
    let cookie = session_cookie(&token, w.lifetime, over_https(&headers))?;
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}

/// End the session whose cookie comes with the request, and clear the cookie.
#[utoipa::path(post, path = "/api/logout", tag = "auth", operation_id = "logout",
    responses(
        (status = 204, description = "Logged out; the session cookie is cleared (also without a live session)"),
    ))]
pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(auth) = state.auth.clone() {
        let hashes: Vec<Vec<u8>> = session_cookies(&headers)
            .iter()
            .map(|t| auth.token_hash(t))
            .collect();
        state
            .blocking(move |db| {
                let sessions = db.sessions();
                hashes.iter().try_for_each(|hash| sessions.delete(hash))
            })
            .await?;
    }
    clearing_cookie(StatusCode::NO_CONTENT, &headers)
}

/// End every session, this one included. Bearer tokens keep working.
#[utoipa::path(post, path = "/api/sessions/revoke-all", tag = "auth",
    operation_id = "revokeAllSessions",
    responses(
        (status = 204, description = "Every session ended; this client's cookie is cleared"),
        (status = 401, description = "Not authorized", body = Problem),
    ))]
pub(crate) async fn revoke_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if state.auth.is_some() {
        state.blocking(|db| db.sessions().delete_all()).await?;
    }
    clearing_cookie(StatusCode::NO_CONTENT, &headers)
}
