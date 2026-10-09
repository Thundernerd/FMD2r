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
    /// Whether the server listens on a loopback address only, out of other machines' reach.
    loopback: bool,
    /// The settings the command line or environment overrides (`--bind`, `--password`, …), as
    /// dotted paths such as `server.bind`.
    overridden: Vec<&'static str>,
}

/// Liveness probe; never requires auth.
#[utoipa::path(get, path = "/api/health", tag = "system", responses((status = 200, body = Health)))]
pub(crate) async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        auth: state.secret().is_some(),
        loopback: state.listen_addr.is_none_or(|a| a.ip().is_loopback()),
        overridden: state
            .overridden
            .iter()
            .copied()
            .chain(state.auth.is_fixed().then_some("server.auth_token"))
            .collect(),
    })
}
