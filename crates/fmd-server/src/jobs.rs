//! `/api/jobs`: the registered background jobs, and `POST /api/modules/update`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::jobs::{Job, JobError};
use fmd_core::module_updater::ModuleUpdaterJob;

use crate::events::JobState;
use crate::state::off_thread;
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

/// One registered job with its state, e.g. `modules` for the module updater.
#[utoipa::path(get, path = "/api/jobs/{id}", tag = "system", operation_id = "getJob",
    params(("id" = String, Path, description = "Job id")),
    responses(
        (status = 200, body = JobState),
        (status = 404, description = "No such job", body = Problem),
    ))]
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobState>, ApiError> {
    let job = state.jobs.get(&id).ok_or(ApiError::NotFound)?;
    Ok(Json(JobState::of(job.as_ref())))
}

/// Sync the Lua modules with upstream now and hot-reload the ones that changed: runs the
/// `modules` job.
#[utoipa::path(post, path = "/api/modules/update", tag = "system", operation_id = "updateModules",
    responses(
        (status = 202, description = "Started; progress follows as `job.state` events", body = JobState),
        (status = 404, description = "The module updater is not running in this server", body = Problem),
        (status = 409, description = "Already running", body = Problem),
    ))]
pub(crate) async fn update_modules(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<JobState>), ApiError> {
    control(&state, ModuleUpdaterJob::ID, |job| job.run()).await
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

/// Off the async threads: a job may take a lock to start or stop.
async fn control(
    state: &AppState,
    id: &str,
    action: fn(&dyn Job) -> Result<(), JobError>,
) -> Result<(StatusCode, Json<JobState>), ApiError> {
    let job = state.jobs.get(id).ok_or(ApiError::NotFound)?;
    let registry = state.jobs.clone();
    let job_state = off_thread(move || {
        action(job.as_ref())?;
        registry.changed(job.id());
        Ok::<_, JobError>(JobState::of(job.as_ref()))
    })
    .await??;
    Ok((StatusCode::ACCEPTED, Json(job_state)))
}
