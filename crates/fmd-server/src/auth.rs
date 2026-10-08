//! Optional single-user auth: a configured password/token, sent as `Authorization: Bearer` or
//! traded for a session cookie at `POST /api/login`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::{ApiError, AppState, Problem};

const COOKIE: &str = "fmd2r_session";

/// The configured secret and the sessions issued for it. Sessions live until the server stops.
pub(crate) struct Auth {
    secret: String,
    sessions: Mutex<HashSet<String>>,
}

impl Auth {
    pub(crate) fn new(secret: String) -> Arc<Self> {
        Arc::new(Self {
            secret,
            sessions: Mutex::new(HashSet::new()),
        })
    }

    fn is_secret(&self, candidate: &str) -> bool {
        bool::from(candidate.as_bytes().ct_eq(self.secret.as_bytes()))
    }

    fn is_session(&self, candidate: &str) -> bool {
        let Ok(sessions) = self.sessions.lock() else {
            return false;
        };
        sessions.iter().fold(false, |hit, s| {
            hit | bool::from(candidate.as_bytes().ct_eq(s.as_bytes()))
        })
    }

    fn new_session(&self) -> Result<String, ApiError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| ApiError::Internal(e.to_string()))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.sessions
            .lock()
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .insert(id.clone());
        Ok(id)
    }

    fn authorizes(&self, headers: &HeaderMap) -> bool {
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        if bearer.is_some_and(|t| self.is_secret(t)) {
            return true;
        }
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('='))
            .any(|id| self.is_session(id))
    }
}

/// Middleware for protected routes: a no-op unless auth is configured.
pub(crate) async fn require(State(state): State<AppState>, req: Request, next: Next) -> Response {
    match &state.auth {
        Some(auth) if !auth.authorizes(req.headers()) => ApiError::Unauthorized.into_response(),
        _ => next.run(req).await,
    }
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
    ApiJson(login): ApiJson<Login>,
) -> Result<Response, ApiError> {
    let Some(auth) = &state.auth else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    if !auth.is_secret(&login.password) {
        return Err(ApiError::Unauthorized);
    }
    let cookie = format!(
        "{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict",
        auth.new_session()?
    );
    let cookie = HeaderValue::from_str(&cookie).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}
