//! `GET /api/health`.

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;

#[derive(Serialize, ToSchema)]
pub(crate) struct Health {
    /// Always `ok` while the server answers.
    status: &'static str,
    /// Whether the API requires the password (as a bearer token or a login session).
    auth: bool,
}

/// Liveness probe; never requires auth.
#[utoipa::path(get, path = "/api/health", tag = "system", responses((status = 200, body = Health)))]
pub(crate) async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        auth: state.auth.is_some(),
    })
}
