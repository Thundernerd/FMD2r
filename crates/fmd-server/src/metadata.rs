//! `/api/metadata/mangabaka`: the opt-in local copy of MangaBaka's database that list titles are
//! matched against.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use fmd_core::metadata::{MetadataEvent, MetadataJobError, MetadataJobs};
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

/// The MangaBaka database at a glance.
#[derive(Debug, Serialize, ToSchema)]
pub struct MangaBakaStatus {
    /// Whether this server can download one (it runs the database job).
    pub available: bool,
    /// Whether one is downloaded.
    pub downloaded: bool,
    /// RFC 3339 time it was built.
    pub built_at: Option<String>,
    /// Its size on disk.
    pub bytes: Option<u64>,
    /// Whether a download or refresh is running.
    pub running: bool,
    /// The running download's last step.
    pub progress: Option<MetadataEvent>,
    /// RFC 3339 time of the next automatic refresh (`metadata.mangabaka.refresh_days`).
    pub next_refresh: Option<String>,
}

impl From<MetadataJobError> for ApiError {
    fn from(err: MetadataJobError) -> Self {
        match err {
            MetadataJobError::AlreadyRunning | MetadataJobError::NotRunning => {
                Self::Conflict(err.to_string())
            }
            MetadataJobError::Spawn(_) | MetadataJobError::Metadata(_) => {
                Self::Internal(err.to_string())
            }
        }
    }
}

fn jobs(state: &AppState) -> Result<MetadataJobs, ApiError> {
    state
        .metadata
        .clone()
        .ok_or_else(|| ApiError::Unavailable("the MangaBaka database is not available".into()))
}

/// The MangaBaka database's date, size and download progress.
#[utoipa::path(get, path = "/api/metadata/mangabaka", tag = "metadata",
    operation_id = "mangabakaStatus",
    responses((status = 200, body = MangaBakaStatus)))]
pub(crate) async fn status(
    State(state): State<AppState>,
) -> Result<Json<MangaBakaStatus>, ApiError> {
    let Some(jobs) = state.metadata.clone() else {
        return Ok(Json(MangaBakaStatus {
            available: false,
            downloaded: false,
            built_at: None,
            bytes: None,
            running: false,
            progress: None,
            next_refresh: None,
        }));
    };
    // Reads the database file.
    let status = off_thread(move || {
        let info = jobs.info();
        let next_refresh = fmd_core::jobs::Job::status(&jobs).next_run;
        MangaBakaStatus {
            available: true,
            downloaded: info.is_some(),
            built_at: info.map(|i| crate::time::rfc3339_from_unix_ms(i.built_at)),
            bytes: info.map(|i| i.bytes),
            running: jobs.is_running(),
            progress: jobs.progress(),
            next_refresh: next_refresh.map(crate::time::rfc3339_from_unix_ms),
        }
    })
    .await?;
    Ok(Json(status))
}

/// Download the MangaBaka database (about 390 MB), or update it, then match every list
/// against it.
#[utoipa::path(post, path = "/api/metadata/mangabaka/download", tag = "metadata",
    operation_id = "downloadMangabaka",
    responses(
        (status = 202, description = "Started; progress follows as `job.metadata.*` events"),
        (status = 409, description = "Already downloading", body = Problem),
        (status = 503, description = "This server cannot download it", body = Problem),
    ))]
pub(crate) async fn download(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    let jobs = jobs(&state)?;
    // Starting spawns a thread.
    off_thread(move || jobs.download()).await??;
    Ok(StatusCode::ACCEPTED)
}

/// Stop the running download; the database it would have replaced stays.
#[utoipa::path(post, path = "/api/metadata/mangabaka/cancel", tag = "metadata",
    operation_id = "cancelMangabaka",
    responses(
        (status = 202, description = "Cancelling; a `job.metadata.cancelled` event follows"),
        (status = 409, description = "Not downloading", body = Problem),
        (status = 503, description = "This server cannot download it", body = Problem),
    ))]
pub(crate) async fn cancel(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    jobs(&state)?.cancel()?;
    Ok(StatusCode::ACCEPTED)
}

/// Delete the MangaBaka database and the list titles' matches in it.
#[utoipa::path(delete, path = "/api/metadata/mangabaka", tag = "metadata",
    operation_id = "removeMangabaka",
    responses(
        (status = 204, description = "Removed"),
        (status = 409, description = "A download is running", body = Problem),
        (status = 503, description = "This server cannot download it", body = Problem),
    ))]
pub(crate) async fn remove(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    let jobs = jobs(&state)?;
    off_thread(move || jobs.remove()).await??;
    Ok(StatusCode::NO_CONTENT)
}
