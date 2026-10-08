//! `GET /api/health`.

use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub(crate) struct Health {
    /// Always `ok` while the server answers.
    status: &'static str,
}

/// Liveness probe; never requires auth.
#[utoipa::path(get, path = "/api/health", tag = "system", responses((status = 200, body = Health)))]
pub(crate) async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}
