//! axum: REST (OpenAPI via utoipa) + SSE event stream + cover proxy/cache + embedded SPA (rust-embed).

mod about;
mod accounts;
mod auth;
mod covers;
mod error;
mod events;
mod favorites;
mod health;
mod inbox;
mod jobs;
mod lists;
mod logs;
mod module_settings;
mod module_updates;
mod series;
mod serve;
mod services;
mod settings;
mod spa;
mod state;
mod task_events;
mod task_files;
mod tasks;
mod time;
mod tools;

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router, middleware};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub use accounts::{AccountInfo, AccountRequest, AccountState, AccountStateChange};
pub use covers::{
    CoverConfig, CoverModules, CoverResolver, CoverSession, SystemResolver, cover_url,
};
pub use error::{ApiError, Problem};
pub use events::{
    EventBus, JobState, ServerEvent, TaskProgress, TaskRemoved, TaskState, TaskStatusChange,
    TasksReordered,
};
pub use favorites::{AddFavorite, CheckRequest, FavoriteFilter, FavoritePatch, FavoriteView};
pub use fmd_core::jobs::JobPhase;
pub use fmd_core::lists::{ListEvent, ListEventKind};
pub use inbox::{InboxItem, InboxKind};
pub use lists::{FacetValue, ListFacets, ListItem, ListJobStarted, SearchPage};
pub use logs::{LogBuffer, LogFilter, LogLevel, LogLine};
pub use module_settings::{ModuleOptionSetting, ModuleSettingsView, ModuleSummary};
pub use series::{ChapterInfo, ResolveRequest, SeriesInfo, SeriesRef, SeriesStatus};
pub use serve::{ServeConfig, ServeError, serve};
pub use services::{
    DownloadEngine, FavoritesJobs, Idle, LoadFailure, ModuleCatalog, ModulesReport,
};
pub use settings::RenamePreview;
pub use spa::{Assets, EmbeddedAssets};
pub use state::AppState;
pub use tasks::{
    ChapterState, NewTask, NewTaskChapter, TaskChapterView, TaskCounts, TaskDetail, TaskGroup,
    TaskList, TaskOrder, TaskSort, TaskSummary,
};
pub use tools::{SystemTools, ToolCheck, ToolProbe};

#[derive(OpenApi)]
#[openapi(
    info(title = "FMD2r"),
    components(schemas(
        TaskProgress,
        TaskStatusChange,
        TaskRemoved,
        TasksReordered,
        TaskGroup,
        TaskSort,
        JobState,
        InboxItem,
        LogLine,
        ListEvent,
        fmd_core::favorites::FavoritesEvent,
        AccountStateChange
    ))
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
        .routes(routes!(jobs::list))
        .routes(routes!(jobs::get))
        .routes(routes!(jobs::update_modules))
        .routes(routes!(jobs::run))
        .routes(routes!(jobs::cancel))
        .routes(routes!(about::about))
        .routes(routes!(covers::get))
        .routes(routes!(settings::get, settings::patch))
        .routes(routes!(settings::preview_rename))
        .routes(routes!(module_settings::list))
        .routes(routes!(module_settings::get, module_settings::patch))
        .routes(routes!(series::resolve))
        .routes(routes!(series::get))
        .routes(routes!(lists::search))
        .routes(routes!(lists::facets))
        .routes(routes!(lists::update))
        .routes(routes!(lists::import_db))
        .routes(routes!(lists::cancel))
        .routes(routes!(favorites::list, favorites::add))
        .routes(routes!(favorites::patch, favorites::delete))
        .routes(routes!(favorites::check))
        .routes(routes!(favorites::check_missing))
        .routes(routes!(accounts::list))
        .routes(routes!(accounts::put, accounts::delete))
        .routes(routes!(accounts::login))
        .routes(routes!(tasks::list, tasks::create, tasks::remove_finished))
        .routes(routes!(tasks::get, tasks::delete))
        .routes(routes!(tasks::start))
        .routes(routes!(tasks::stop))
        .routes(routes!(tasks::redownload))
        .routes(routes!(tasks::enable))
        .routes(routes!(tasks::disable))
        .routes(routes!(tasks::start_all))
        .routes(routes!(tasks::stop_all))
        .routes(routes!(tasks::reorder))
        .routes(routes!(task_files::get))
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
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(spa::serve)
        .with_state(state)
}

async fn api_not_found() -> ApiError {
    ApiError::NotFound
}

async fn method_not_allowed() -> ApiError {
    ApiError::MethodNotAllowed
}

/// The OpenAPI 3.1 document of the REST API, as served at `/api/openapi.json`.
pub fn openapi() -> utoipa::openapi::OpenApi {
    api().2
}

/// [`openapi`] as pretty-printed JSON, for exporting to the web client generator.
pub fn openapi_json() -> Result<String, serde_json::Error> {
    openapi().to_pretty_json()
}
