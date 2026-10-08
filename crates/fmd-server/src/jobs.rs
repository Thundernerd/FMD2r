//! `GET /api/jobs`, `POST /api/jobs/{id}/run` and `POST /api/jobs/{id}/cancel`: the registered
//! background jobs, for the System page.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::jobs::{Job, JobError};

use crate::events::JobState;
use crate::{ApiError, AppState, Problem};

/// Every registered background job with its state, in registration order.
#[utoipa::path(get, path = "/api/jobs", tag = "system", operation_id = "listJobs",
    responses((status = 200, body = Vec<JobState>)))]
pub(crate) async fn list(State(state): State<AppState>) -> Json<Vec<JobState>> {
    Json(
        state
            .jobs
            .list()
            .iter()
            .map(|j| JobState::of(j.as_ref()))
            .collect(),
    )
}

/// Start a job now.
#[utoipa::path(post, path = "/api/jobs/{id}/run", tag = "system", operation_id = "runJob",
    params(("id" = String, Path, description = "Job id")),
    responses(
        (status = 202, description = "Started; progress follows as `job.state` events", body = JobState),
        (status = 404, description = "No such job", body = Problem),
        (status = 409, description = "Already running", body = Problem),
    ))]
pub(crate) async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<JobState>), ApiError> {
    control(&state, &id, |job| job.run()).await
}

/// Ask a running job to stop.
#[utoipa::path(post, path = "/api/jobs/{id}/cancel", tag = "system", operation_id = "cancelJob",
    params(("id" = String, Path, description = "Job id")),
    responses(
        (status = 202, description = "Cancelling; the outcome follows as `job.state` events", body = JobState),
        (status = 404, description = "No such job", body = Problem),
        (status = 409, description = "Not running", body = Problem),
    ))]
pub(crate) async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<JobState>), ApiError> {
    control(&state, &id, |job| job.cancel()).await
}

/// Applies `action` to job `id` off the async threads (a job may take a lock to start or stop)
/// and answers with its state afterwards.
async fn control(
    state: &AppState,
    id: &str,
    action: fn(&dyn Job) -> Result<(), JobError>,
) -> Result<(StatusCode, Json<JobState>), ApiError> {
    let job = state.jobs.get(id).ok_or(ApiError::NotFound)?;
    let registry = state.jobs.clone();
    let job_state = tokio::task::spawn_blocking(move || {
        action(job.as_ref())?;
        registry.changed(job.id());
        Ok::<_, JobError>(JobState::of(job.as_ref()))
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok((StatusCode::ACCEPTED, Json(job_state)))
}
