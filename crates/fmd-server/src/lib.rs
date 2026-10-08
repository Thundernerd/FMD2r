//! axum: REST (OpenAPI via utoipa) + SSE event stream + cover proxy/cache + embedded SPA (rust-embed).

mod auth;
mod error;
mod events;
mod health;
mod inbox;
mod logs;
mod serve;
mod services;
mod settings;
mod spa;
mod state;
mod time;

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router, middleware};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub use error::{ApiError, Problem};
pub use events::{
    EventBus, JobPhase, JobState, ServerEvent, TaskProgress, TaskState, TaskStatusChange,
};
pub use inbox::{InboxItem, InboxKind};
pub use logs::{LogBuffer, LogLevel, LogLine};
pub use serve::{ServeConfig, ServeError, serve};
pub use services::{DownloadEngine, Idle, Jobs};
pub use settings::{SettingsError, SettingsService, StoreSettings};
pub use spa::{Assets, EmbeddedAssets};
pub use state::AppState;

#[derive(OpenApi)]
#[openapi(
    info(title = "FMD2r"),
    components(schemas(TaskProgress, TaskStatusChange, JobState, InboxItem, LogLine))
)]
struct ApiDoc;

/// Routes reachable without auth.
fn public_api() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health::health))
        .routes(routes!(auth::login))
}

/// Routes behind the auth layer (when auth is configured).
fn protected_api() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(events::stream))
        .routes(routes!(inbox::list))
        .routes(routes!(inbox::mark_read))
        .routes(routes!(logs::list))
        .routes(routes!(settings::get, settings::patch))
}

/// The public and protected `/api` routers plus the OpenAPI document describing both.
fn api() -> (Router<AppState>, Router<AppState>, utoipa::openapi::OpenApi) {
    let (public, mut doc) = public_api().split_for_parts();
    let (protected, protected_doc) = protected_api().split_for_parts();
    doc.merge(protected_doc);
    (public, protected, doc)
}

/// The whole HTTP app: the REST API under `/api` and the web UI everywhere else.
pub fn build_router(state: AppState) -> Router {
    let (public, protected, doc) = api();
    let doc = Arc::new(doc);
    let protected =
        protected.route_layer(middleware::from_fn_with_state(state.clone(), auth::require));
    public
        .merge(protected)
        .route(
            "/api/openapi.json",
            get(move || async move { Json(doc.as_ref().clone()) }),
        )
        .nest("/api", Router::new().fallback(api_not_found))
        .fallback(spa::serve)
        .with_state(state)
}

async fn api_not_found() -> ApiError {
    ApiError::NotFound
}

/// The OpenAPI 3.1 document of the REST API, as served at `/api/openapi.json`.
pub fn openapi() -> utoipa::openapi::OpenApi {
    api().2
}

/// [`openapi`] as pretty-printed JSON, for exporting to the web client generator.
pub fn openapi_json() -> String {
    openapi().to_pretty_json().unwrap_or_default()
}
