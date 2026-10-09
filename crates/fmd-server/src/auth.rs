//! Optional single-user auth: a configured password/token, sent as `Authorization: Bearer` or
//! traded for a session cookie at `POST /api/login`.
//!
//! Sessions live in `app.db` and end after `server.session_idle_days` without use, after
//! `server.session_lifetime_days` in any case, at `POST /api/logout`, at
//! `POST /api/sessions/revoke-all`, and when the password changes. No FMD2 counterpart: FMD2 has
//! no web server.

use std::sync::{Arc, Mutex};

use std::time::{Duration, SystemTime};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use fmd_core::settings::{Settings, verify_password};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

const COOKIE: &str = "fmd2r_session";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// A session's last use is written at most this often, not on every request.
const RENEW_EVERY: Duration = Duration::from_secs(60);

/// Bearer tokens remembered as verified, at most.
const VERIFIED_TOKENS: usize = 16;
/// Password hashes checked at once, at most: each takes tens of milliseconds and ~19 MiB, and
/// any client can ask for one.
const CONCURRENT_HASHES: usize = 2;

/// Where the password comes from: `--password` / `FMD2R_PASSWORD` when given, which wins, else
/// the `server.auth_token` setting (a hash), read at every request so a change applies at once.
pub(crate) struct Auth {
    fixed: Option<String>,
    verified: Mutex<Verified>,
    hashing: Semaphore,
}

/// Bearer tokens verified against the setting's hash, so a request does not pay for hashing.
#[derive(Default)]
struct Verified {
    /// The hash they were verified against; a new password forgets them.
    hash: String,
    /// SHA-256 digests of the tokens, newest last.
    tokens: Vec<[u8; 32]>,
}

impl Auth {
    /// Auth from the setting only.
    pub(crate) fn from_settings() -> Arc<Self> {
        Arc::new(Self::new(None))
    }

    /// Auth with `secret` from the command line or environment, ignoring the setting.
    pub(crate) fn fixed(secret: String) -> Arc<Self> {
        Arc::new(Self::new(Some(secret)))
    }

    fn new(fixed: Option<String>) -> Self {
        Self {
            fixed,
            verified: Mutex::default(),
            hashing: Semaphore::new(CONCURRENT_HASHES),
        }
    }

    /// Whether the command line or environment sets the password.
    pub(crate) fn is_fixed(&self) -> bool {
        self.fixed.is_some()
    }

    /// The password requests need now, or `None` when the API is open.
    pub(crate) fn current(&self, settings: &Settings) -> Option<Secret> {
        match &self.fixed {
            Some(secret) => Some(Secret::Plain(secret.clone())),
            None => settings.server.auth_token.clone().map(Secret::Hash),
        }
    }

    /// Whether `candidate` is the password. A hash is checked on the blocking pool, a few at a
    /// time.
    async fn matches(&self, secret: &Secret, candidate: &str) -> Result<bool, ApiError> {
        match secret {
            Secret::Plain(secret) => Ok(ct_eq(candidate, secret)),
            Secret::Hash(hash) => {
                let _permit = self
                    .hashing
                    .acquire()
                    .await
                    .map_err(|e| ApiError::Internal(e.to_string()))?;
                let (hash, candidate) = (hash.clone(), candidate.to_owned());
                off_thread(move || verify_password(&hash, &candidate)).await
            }
        }
    }

    /// Whether the request's bearer token is known to be `secret` without hashing: it is the
    /// command line one, or one verified before.
    fn bearer_known(&self, secret: &Secret, headers: &HeaderMap) -> bool {
        let Some(token) = bearer_token(headers) else {
            return false;
        };
        match secret {
            Secret::Plain(secret) => ct_eq(token, secret),
            Secret::Hash(hash) => {
                let digest = token_digest(token);
                self.verified(|v| v.hash == *hash && v.tokens.contains(&digest))
            }
        }
    }

    /// Whether the request's bearer token is the password `hash` was made from; remembers it
    /// when it is.
    async fn bearer_verifies(
        &self,
        secret: &Secret,
        headers: &HeaderMap,
    ) -> Result<bool, ApiError> {
        let (Some(token), Secret::Hash(hash)) = (bearer_token(headers), secret) else {
            return Ok(false);
        };
        if !self.matches(secret, token).await? {
            return Ok(false);
        }
        let digest = token_digest(token);
        self.verified(|v| {
            if v.hash != *hash {
                *v = Verified {
                    hash: hash.clone(),
                    tokens: Vec::new(),
                };
            }
            if v.tokens.len() >= VERIFIED_TOKENS {
                v.tokens.remove(0);
            }
            v.tokens.push(digest);
        });
        Ok(true)
    }

    fn verified<T>(&self, f: impl FnOnce(&mut Verified) -> T) -> T {
        f(&mut self.verified.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// What [`Verified`] keeps of a bearer token.
fn token_digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// The password a request is checked against.
pub(crate) enum Secret {
    /// From the command line or environment.
    Plain(String),
    /// The `server.auth_token` setting: a hash of the password.
    Hash(String),
}

impl Secret {
    /// What sessions are bound to: the password, or the stored hash (with its own salt, so
    /// setting a password again also ends them).
    fn binding(&self) -> &str {
        match self {
            Secret::Plain(s) | Secret::Hash(s) => s,
        }
    }

    /// What `app.db` stores for the session cookie `token`: a hash bound to the secret, so a
    /// password change leaves every stored session unmatched.
    fn token_hash(&self, token: &str) -> Vec<u8> {
        let binding = self.binding();
        let mut hash = Sha256::new();
        hash.update(b"fmd2r-session\0");
        hash.update((binding.len() as u64).to_le_bytes());
        hash.update(binding.as_bytes());
        hash.update(token.as_bytes());
        hash.finalize().to_vec()
    }

    /// The stored hashes of the session cookies sent with a request.
    fn session_hashes(&self, headers: &HeaderMap) -> Vec<Vec<u8>> {
        session_cookies(headers)
            .iter()
            .map(|t| self.token_hash(t))
            .collect()
    }
}

/// The token of an `Authorization: Bearer` header.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim())
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

/// A fresh random session token (256 bits, hex).
fn new_token() -> Result<String, ApiError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Unix milliseconds of `at`, saturating.
fn unix_ms(at: SystemTime) -> i64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The session limits as of now (Unix ms): the oldest last use and creation a live session may
/// have, and how long a new session lasts at most.
struct SessionCutoffs {
    now: i64,
    seen_since: i64,
    created_since: i64,
    lifetime: Duration,
}

impl SessionCutoffs {
    fn current(state: &AppState) -> Self {
        let now = unix_ms(state.now());
        let server = &state.settings.get().server;
        let ms = |d: Duration| i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
        let idle = DAY.saturating_mul(server.session_idle_days);
        let lifetime = DAY.saturating_mul(server.session_lifetime_days);
        Self {
            now,
            seen_since: now.saturating_sub(ms(idle)),
            created_since: now.saturating_sub(ms(lifetime)),
            lifetime,
        }
    }

    /// Last uses older than this are rewritten on the next request.
    fn renew_before(&self) -> i64 {
        self.now
            .saturating_sub(i64::try_from(RENEW_EVERY.as_millis()).unwrap_or(i64::MAX))
    }
}

/// Whether a session cookie in `headers` names a live session; renews the ones that do.
async fn session_authorizes(
    state: &AppState,
    secret: &Secret,
    headers: &HeaderMap,
) -> Result<bool, ApiError> {
    let hashes = secret.session_hashes(headers);
    if hashes.is_empty() {
        return Ok(false);
    }
    let c = SessionCutoffs::current(state);
    state
        .blocking(move |db| {
            let sessions = db.sessions();
            let mut live = false;
            for hash in &hashes {
                live |=
                    sessions.renew(hash, c.now, c.renew_before(), c.seen_since, c.created_since)?;
            }
            Ok::<_, fmd_store::StoreError>(live)
        })
        .await
}

/// Middleware for protected routes: a no-op unless auth is configured.
pub(crate) async fn require(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let Some(secret) = state.secret() else {
        return next.run(req).await;
    };
    if state.auth.bearer_known(&secret, req.headers()) {
        return next.run(req).await;
    }
    // A session before an unknown bearer token, which costs a hash.
    let authorized = match session_authorizes(&state, &secret, req.headers()).await {
        Ok(true) => Ok(true),
        Ok(false) => state.auth.bearer_verifies(&secret, req.headers()).await,
        Err(e) => Err(e),
    };
    match authorized {
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
    let Some(secret) = state.secret() else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    if !state.auth.matches(&secret, &login.password).await? {
        return Err(ApiError::Unauthorized);
    }
    let token = new_token()?;
    let hash = secret.token_hash(&token);
    let c = SessionCutoffs::current(&state);
    state
        .blocking(move |db| {
            let sessions = db.sessions();
            sessions.delete_expired(c.seen_since, c.created_since)?;
            sessions.create(&hash, c.now)
        })
        .await?;
    let cookie = session_cookie(&token, c.lifetime, over_https(&headers))?;
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
    if let Some(secret) = state.secret() {
        let hashes = secret.session_hashes(&headers);
        state
            .blocking(move |db| {
                let sessions = db.sessions();
                hashes.iter().try_for_each(|hash| sessions.delete(hash))
            })
            .await?;
        state.end_sessions();
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
    if state.secret().is_some() {
        state.blocking(|db| db.sessions().delete_all()).await?;
        state.end_sessions();
    }
    clearing_cookie(StatusCode::NO_CONTENT, &headers)
}
