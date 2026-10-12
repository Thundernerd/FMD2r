//! `PUT` and `DELETE /api/series/match`: matching a series to MangaBaka by hand (issue #249).

use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiJson, ApiQuery};
use crate::metadata::jobs;
use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

/// The MangaBaka series a series is.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MatchRequest {
    /// Module ID.
    pub module: String,
    /// The series link relative to the module's `RootURL`, as `GET /api/series` reports it.
    pub link: String,
    /// The MangaBaka series ID.
    pub series_id: i64,
}

/// Which series to match automatically again.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct MatchQuery {
    /// Module ID.
    module: String,
    /// The series link relative to the module's `RootURL`.
    link: String,
}

/// Match a series to a MangaBaka series by hand. The match outlives list updates, matching,
/// database refreshes and removing the database.
#[utoipa::path(put, path = "/api/series/match", tag = "series", operation_id = "setSeriesMatch",
    request_body = MatchRequest,
    responses(
        (status = 204, description = "Matched"),
        (status = 404, body = Problem, description = "The database has no such MangaBaka series"),
        (status = 503, body = Problem, description = "No MangaBaka database is downloaded"),
    ))]
pub(crate) async fn set_match(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<MatchRequest>,
) -> Result<StatusCode, ApiError> {
    let jobs = jobs(&state)?;
    off_thread(move || jobs.set_override(&req.module, &req.link, req.series_id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

/// Undo a match by hand: the series is matched automatically again.
#[utoipa::path(delete, path = "/api/series/match", tag = "series",
    operation_id = "removeSeriesMatch",
    params(MatchQuery),
    responses(
        (status = 204, description = "Matched automatically again, if it can be"),
        (status = 503, body = Problem, description = "This server has no MangaBaka database job"),
    ))]
pub(crate) async fn remove_match(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<MatchQuery>,
) -> Result<StatusCode, ApiError> {
    let jobs = jobs(&state)?;
    off_thread(move || jobs.remove_override(&query.module, &query.link)).await??;
    Ok(StatusCode::NO_CONTENT)
}
