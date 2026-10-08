//! The download queue (`/api/tasks*`): FMD2's downloads tab and the `TDownloadManager`
//! operations behind its menu (baseunits/uDownloadsManager.pas:1769-2045).

use std::cmp::Ordering;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::download::{
    ChapterSpec, ChapterStatus, EngineError, NewDownload, Progress, TaskId, TaskInfo, TaskStatus,
};
use fmd_pack::natural_cmp;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiJson, ApiQuery};
use crate::events::TaskState;
use crate::time::rfc3339_from_unix_ms;
use crate::{ApiError, AppState, Problem};

/// A task as the queue lists it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskSummary {
    pub id: i64,
    pub module_id: String,
    /// The series link relative to the module's `RootURL`.
    pub link: String,
    pub title: String,
    /// The folder the chapters are saved in.
    pub save_to: String,
    pub status: TaskState,
    /// Disabled tasks are skipped by start and start-all.
    pub enabled: bool,
    /// Whether the task's thread runs (it may still show Waiting until its first chapter starts).
    pub running: bool,
    /// Why the task failed.
    pub error: Option<String>,
    /// The chapter the task is at, e.g. `Ch. 1101 (2/2)`.
    pub chapters: String,
    pub chapter_count: u32,
    /// Chapters downloaded.
    pub chapters_done: u32,
    /// Index of the chapter the task is at.
    pub current_chapter: u32,
    /// Pages of the current chapter done (FMD2's `DownCounter`).
    pub done: u64,
    /// Pages of the current chapter.
    pub total: u64,
    /// Download speed; 0 when not running.
    pub bytes_per_sec: f64,
    /// RFC 3339.
    pub date_added: String,
    /// RFC 3339 end of the last download run.
    pub date_last_downloaded: Option<String>,
}

/// A task with its chapters.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskDetail {
    pub task: TaskSummary,
    /// In download order.
    pub chapters: Vec<TaskChapterView>,
}

/// One chapter of a task.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskChapterView {
    pub index: u32,
    pub name: String,
    /// Relative to the module's `RootURL`.
    pub link: String,
    pub status: ChapterState,
    /// Pages done.
    pub done: u64,
    /// Pages in total; 0 until the page count is known.
    pub total: u64,
}

/// A chapter's status, FMD2's `ChaptersStatus` markers (baseunits/uDownloadsManager.pas:1117-1123).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChapterState {
    Pending,
    Downloaded,
    Failed,
}

impl From<ChapterStatus> for ChapterState {
    fn from(status: ChapterStatus) -> Self {
        match status {
            ChapterStatus::Pending => Self::Pending,
            ChapterStatus::Downloaded => Self::Downloaded,
            ChapterStatus::Failed => Self::Failed,
        }
    }
}

/// A task's last progress report, while it runs; a stopped task's is stale.
fn live(info: &TaskInfo) -> Option<Progress> {
    info.progress.filter(|_| info.running)
}

impl From<&TaskInfo> for TaskDetail {
    fn from(info: &TaskInfo) -> Self {
        let live = live(info);
        TaskDetail {
            task: TaskSummary::from(info),
            chapters: info
                .chapters
                .iter()
                .map(|c| {
                    let (done, total) = match live {
                        Some(p) if p.chapter == c.idx => (p.pages_done, p.pages_total),
                        _ => (c.current_page, c.page_count),
                    };
                    TaskChapterView {
                        index: c.idx,
                        name: c.name.clone(),
                        link: c.link.clone(),
                        status: c.status.into(),
                        done: u64::from(done),
                        total: u64::from(total),
                    }
                })
                .collect(),
        }
    }
}

/// Tasks per status group, for the queue's group headers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, ToSchema)]
pub struct TaskCounts {
    /// Preparing, downloading, converting or compressing.
    pub downloading: u64,
    pub waiting: u64,
    /// Stopped, failed or disabled.
    pub stopped: u64,
    pub finished: u64,
}

/// One page of the queue.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskList {
    pub items: Vec<TaskSummary>,
    /// Tasks matching the filter, over all pages.
    pub total: u64,
    /// 1-based.
    pub page: u32,
    pub per_page: u32,
    /// Tasks per group matching the text and date filters (not the status filter).
    pub counts: TaskCounts,
}

/// Tasks per page when the request does not say.
const DEFAULT_PER_PAGE: u32 = 100;
const MAX_PER_PAGE: u32 = 1000;

/// Which tasks to list, in which order.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct TaskQuery {
    /// Only this status group.
    status: Option<TaskGroup>,
    /// Text in the title, the module ID or a chapter name, ignoring case.
    q: Option<String>,
    /// Unix ms: only tasks last downloaded (or, never downloaded, added) at or after this.
    from: Option<i64>,
    /// Unix ms: only tasks last downloaded (or, never downloaded, added) at or before this.
    to: Option<i64>,
    /// 1-based page; default 1.
    page: Option<u32>,
    /// Default 100, at most 1000.
    per_page: Option<u32>,
    /// Default: queue order.
    sort: Option<TaskSort>,
    /// Sort descending.
    #[serde(default)]
    desc: bool,
}

/// The columns FMD2 sorts its downloads list by (`CompareTaskContainer`,
/// baseunits/uDownloadsManager.pas:2046-2083), plus the queue order itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskSort {
    Queue,
    Title,
    Status,
    Progress,
    Speed,
    Website,
    SaveTo,
    Added,
}

/// The status groups of the queue page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TaskGroup {
    Downloading,
    Waiting,
    Stopped,
    Finished,
}

fn group(status: TaskStatus) -> TaskGroup {
    match status {
        TaskStatus::Preparing
        | TaskStatus::Downloading
        | TaskStatus::Converting
        | TaskStatus::Compressing => TaskGroup::Downloading,
        TaskStatus::Waiting => TaskGroup::Waiting,
        TaskStatus::Stopped | TaskStatus::Failed | TaskStatus::Disabled => TaskGroup::Stopped,
        TaskStatus::Finished => TaskGroup::Finished,
    }
}

impl TaskCounts {
    fn add(&mut self, status: TaskStatus) {
        match group(status) {
            TaskGroup::Downloading => self.downloading += 1,
            TaskGroup::Waiting => self.waiting += 1,
            TaskGroup::Stopped => self.stopped += 1,
            TaskGroup::Finished => self.finished += 1,
        }
    }
}

/// The label of the chapter a task is at: its name, plus its position when there are several.
pub(crate) fn chapter_label(names: &[&str], current: u32) -> String {
    let index = (current as usize).min(names.len().saturating_sub(1));
    let name = names.get(index).copied().unwrap_or_default();
    if names.len() > 1 {
        format!("{name} ({}/{})", index + 1, names.len())
    } else {
        name.to_owned()
    }
}

impl From<&TaskInfo> for TaskSummary {
    fn from(info: &TaskInfo) -> Self {
        let task = &info.task;
        let names: Vec<&str> = info.chapters.iter().map(|c| c.name.as_str()).collect();
        let current = info.chapters.get(task.current_chapter as usize);
        let live = live(info);
        let (done, total) = match (live, current) {
            (Some(p), _) => (u64::from(p.pages_done), u64::from(p.pages_total)),
            (None, Some(c)) => (u64::from(c.current_page), u64::from(c.page_count)),
            (None, None) => (0, 0),
        };
        let count = |status| {
            u32::try_from(info.chapters.iter().filter(|c| c.status == status).count())
                .unwrap_or(u32::MAX)
        };
        TaskSummary {
            id: task.id.0,
            module_id: task.module_id.clone(),
            link: task.link.clone(),
            title: task.title.clone(),
            save_to: task.save_to.clone(),
            status: task.status.into(),
            enabled: task.enabled,
            running: info.running,
            error: task.error.clone(),
            chapters: chapter_label(&names, task.current_chapter),
            chapter_count: u32::try_from(names.len()).unwrap_or(u32::MAX),
            chapters_done: count(ChapterStatus::Downloaded),
            current_chapter: task.current_chapter,
            done,
            total,
            bytes_per_sec: live.map_or(0.0, |p| p.bytes_per_sec as f64),
            date_added: rfc3339_from_unix_ms(task.date_added),
            date_last_downloaded: task.date_last_downloaded.map(rfc3339_from_unix_ms),
        }
    }
}

impl From<EngineError> for ApiError {
    fn from(err: EngineError) -> Self {
        match err {
            EngineError::NoTask(_) => Self::NotFound,
            EngineError::NoModule(_) => Self::Invalid {
                field: Some("module_id".into()),
                detail: err.to_string(),
            },
            EngineError::NoChapters => Self::Invalid {
                field: Some("chapters".into()),
                detail: err.to_string(),
            },
            _ => Self::Internal(err.to_string()),
        }
    }
}

/// Whether `info` matches the text and date filters of `query`.
fn matches(info: &TaskInfo, query: &TaskQuery) -> bool {
    let task = &info.task;
    let date = task.date_last_downloaded.unwrap_or(task.date_added);
    if query.from.is_some_and(|from| date < from) || query.to.is_some_and(|to| date > to) {
        return false;
    }
    match query.q.as_deref().map(str::trim) {
        None | Some("") => true,
        Some(q) => {
            let q = q.to_lowercase();
            let has = |text: &str| text.to_lowercase().contains(&q);
            has(&task.title) || has(&task.module_id) || info.chapters.iter().any(|c| has(&c.name))
        }
    }
}

/// `CompareTaskContainer` (baseunits/uDownloadsManager.pas:2046-2083): text columns compare
/// naturally, the date added by time. The speed compares as a number rather than as FMD2's
/// formatted rate text.
fn compare(sort: TaskSort, a: &TaskSummary, b: &TaskSummary) -> Ordering {
    match sort {
        TaskSort::Queue => Ordering::Equal,
        TaskSort::Title => natural_cmp(&a.title, &b.title),
        TaskSort::Status => natural_cmp(status_text(a.status), status_text(b.status)),
        TaskSort::Progress => natural_cmp(
            &format!("{}/{}", a.done, a.total),
            &format!("{}/{}", b.done, b.total),
        ),
        TaskSort::Speed => a.bytes_per_sec.total_cmp(&b.bytes_per_sec),
        TaskSort::Website => natural_cmp(&a.module_id, &b.module_id),
        TaskSort::SaveTo => natural_cmp(&a.save_to, &b.save_to),
        TaskSort::Added => a.date_added.cmp(&b.date_added),
    }
}

fn status_text(status: TaskState) -> &'static str {
    match status {
        TaskState::Stopped => "stopped",
        TaskState::Waiting => "waiting",
        TaskState::Preparing => "preparing",
        TaskState::Downloading => "downloading",
        TaskState::Converting => "converting",
        TaskState::Compressing => "compressing",
        TaskState::Finished => "finished",
        TaskState::Failed => "failed",
        TaskState::Disabled => "disabled",
    }
}

/// The download queue, filtered, sorted and paged.
#[utoipa::path(get, path = "/api/tasks", tag = "tasks", operation_id = "listTasks",
    params(TaskQuery),
    responses(
        (status = 200, body = TaskList, description = "One page of the matching tasks"),
        (status = 400, body = Problem, description = "A malformed query, e.g. page 0"),
    ))]
pub(crate) async fn list(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<TaskQuery>,
) -> Result<Json<TaskList>, ApiError> {
    let page = query.page.unwrap_or(1);
    if page == 0 {
        return Err(ApiError::BadRequest("pages start at 1".into()));
    }
    let per_page = query
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);
    let tasks = state.engine.list().await?;
    let mut counts = TaskCounts::default();
    let mut items = Vec::new();
    for info in tasks.iter().filter(|info| matches(info, &query)) {
        counts.add(info.task.status);
        if query.status.is_none_or(|g| g == group(info.task.status)) {
            items.push(TaskSummary::from(info));
        }
    }
    if let Some(sort) = query.sort {
        // Stable, so ties keep their queue order.
        items.sort_by(|a, b| compare(sort, a, b));
    }
    if query.desc {
        items.reverse();
    }
    let total = items.len() as u64;
    let start = (page as usize - 1).saturating_mul(per_page as usize);
    let items = items
        .into_iter()
        .skip(start)
        .take(per_page as usize)
        .collect();
    Ok(Json(TaskList {
        items,
        total,
        page,
        per_page,
        counts,
    }))
}

/// The task with ID `id`, or a 404.
pub(crate) async fn find(state: &AppState, id: i64) -> Result<TaskInfo, ApiError> {
    state
        .engine
        .list()
        .await?
        .into_iter()
        .find(|info| info.task.id.0 == id)
        .ok_or(ApiError::NotFound)
}

/// A task with its chapters and their progress.
#[utoipa::path(get, path = "/api/tasks/{id}", tag = "tasks", operation_id = "getTask",
    params(("id" = i64, Path, description = "Task ID")),
    responses(
        (status = 200, body = TaskDetail, description = "The task and its chapters"),
        (status = 404, body = Problem, description = "No such task"),
    ))]
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskDetail>, ApiError> {
    Ok(Json(TaskDetail::from(&find(&state, id).await?)))
}

/// A download to queue: chapters of one series.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewTask {
    pub module_id: String,
    /// The series link relative to the module's `RootURL`.
    pub link: String,
    pub title: String,
    /// For `%AUTHOR%` in the folder and chapter names.
    #[serde(default)]
    pub authors: String,
    /// For `%ARTIST%` in the folder and chapter names.
    #[serde(default)]
    pub artists: String,
    /// In download order.
    pub chapters: Vec<NewTaskChapter>,
    /// The folder the series folder is made in; the configured download folder when empty or
    /// missing.
    #[serde(default)]
    pub save_to: String,
}

/// A chapter to download.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewTaskChapter {
    pub name: String,
    /// Relative to the module's `RootURL`.
    pub link: String,
    /// 1-based position in the series' chapter list, for `%NUMBERING%`; the position in
    /// `chapters` when missing.
    pub number: Option<u32>,
}

/// Queue chapters of a series for download.
#[utoipa::path(post, path = "/api/tasks", tag = "tasks", operation_id = "createTask",
    request_body = NewTask,
    responses(
        (status = 201, body = TaskSummary, description = "The queued task"),
        (status = 422, body = Problem, description = "No chapters, or no such module"),
    ))]
pub(crate) async fn create(
    State(state): State<AppState>,
    ApiJson(new): ApiJson<NewTask>,
) -> Result<(StatusCode, Json<TaskSummary>), ApiError> {
    let download = NewDownload {
        module_id: new.module_id,
        manga_link: new.link,
        title: new.title,
        authors: new.authors,
        artists: new.artists,
        chapters: new
            .chapters
            .into_iter()
            .zip(1u32..)
            .map(|(c, position)| ChapterSpec {
                link: c.link,
                title: c.name,
                number: c.number.unwrap_or(position),
            })
            .collect(),
        save_to: new.save_to,
    };
    let id = state.engine.add(download).await?;
    let info = find(&state, id.0).await?;
    Ok((StatusCode::CREATED, Json(TaskSummary::from(&info))))
}

/// Runs `action` on task `id`, then answers the task as it is now.
async fn act<'a>(
    state: &'a AppState,
    id: i64,
    action: impl FnOnce(&'a dyn crate::DownloadEngine, TaskId) -> BoxFuture<'a, Result<(), EngineError>>,
) -> Result<Json<TaskSummary>, ApiError> {
    action(state.engine.as_ref(), TaskId(id)).await?;
    Ok(Json(TaskSummary::from(&find(state, id).await?)))
}

/// Start a stopped or failed task (`SetTaskActive`, baseunits/uDownloadsManager.pas:1835-1844).
#[utoipa::path(post, path = "/api/tasks/{id}/start", tag = "tasks", operation_id = "startTask",
    params(("id" = i64, Path, description = "Task ID")),
    responses(
        (status = 200, body = TaskSummary, description = "The task, now waiting"),
        (status = 404, body = Problem, description = "No such task"),
    ))]
pub(crate) async fn start(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskSummary>, ApiError> {
    act(&state, id, |engine, id| engine.start(id)).await
}

/// Stop a waiting or running task (`StopTask`, baseunits/uDownloadsManager.pas:1900-1920). A
/// running task shows Stopped once its download has ended, which follows as a `task.status`
/// event.
#[utoipa::path(post, path = "/api/tasks/{id}/stop", tag = "tasks", operation_id = "stopTask",
    params(("id" = i64, Path, description = "Task ID")),
    responses(
        (status = 200, body = TaskSummary, description = "The task"),
        (status = 404, body = Problem, description = "No such task"),
    ))]
pub(crate) async fn stop(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskSummary>, ApiError> {
    act(&state, id, |engine, id| engine.stop(id)).await
}

/// Download every chapter of a task again (`RedownloadTask`,
/// baseunits/uDownloadsManager.pas:1846-1857); pages and archives on disk are kept.
#[utoipa::path(post, path = "/api/tasks/{id}/redownload", tag = "tasks", operation_id = "redownloadTask",
    params(("id" = i64, Path, description = "Task ID")),
    responses(
        (status = 200, body = TaskSummary, description = "The task, now waiting"),
        (status = 404, body = Problem, description = "No such task"),
    ))]
pub(crate) async fn redownload(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskSummary>, ApiError> {
    act(&state, id, |engine, id| engine.redownload(id)).await
}

/// Enable a disabled task, which becomes Stopped (`EnableTask`,
/// baseunits/uDownloadsManager.pas:2022-2026).
#[utoipa::path(post, path = "/api/tasks/{id}/enable", tag = "tasks", operation_id = "enableTask",
    params(("id" = i64, Path, description = "Task ID")),
    responses(
        (status = 200, body = TaskSummary, description = "The task"),
        (status = 404, body = Problem, description = "No such task"),
    ))]
pub(crate) async fn enable(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskSummary>, ApiError> {
    act(&state, id, |engine, id| engine.enable(id)).await
}

/// Stop a task and disable it, so start-all skips it (`DisableTask`,
/// baseunits/uDownloadsManager.pas:2028-2045).
#[utoipa::path(post, path = "/api/tasks/{id}/disable", tag = "tasks", operation_id = "disableTask",
    params(("id" = i64, Path, description = "Task ID")),
    responses(
        (status = 200, body = TaskSummary, description = "The task"),
        (status = 404, body = Problem, description = "No such task"),
    ))]
pub(crate) async fn disable(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskSummary>, ApiError> {
    act(&state, id, |engine, id| engine.disable(id)).await
}

/// Set every task but the finished and disabled ones waiting (`StartAllTasks`,
/// baseunits/uDownloadsManager.pas:1922-1941).
#[utoipa::path(post, path = "/api/tasks/start-all", tag = "tasks", operation_id = "startAllTasks",
    responses((status = 204, description = "Started")))]
pub(crate) async fn start_all(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.engine.start_all().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stop every task (`StopAllTasks`, baseunits/uDownloadsManager.pas:1943-1955).
#[utoipa::path(post, path = "/api/tasks/stop-all", tag = "tasks", operation_id = "stopAllTasks",
    responses((status = 204, description = "Stopped")))]
pub(crate) async fn stop_all(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.engine.stop_all().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A new queue order.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TaskOrder {
    /// These tasks go first, in this order; the others keep theirs after them.
    pub ids: Vec<i64>,
}

/// Reorder the queue; waiting tasks start in queue order.
#[utoipa::path(post, path = "/api/tasks/reorder", tag = "tasks", operation_id = "reorderTasks",
    request_body = TaskOrder,
    responses((status = 204, description = "Reordered")))]
pub(crate) async fn reorder(
    State(state): State<AppState>,
    ApiJson(order): ApiJson<TaskOrder>,
) -> Result<StatusCode, ApiError> {
    state
        .engine
        .reorder(order.ids.into_iter().map(TaskId).collect())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Whether to delete a task's files too.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct DeleteQuery {
    /// Also delete the chapters' folders and archives (default false).
    #[serde(default)]
    files: bool,
}

/// Remove a task, stopping it first (miDownloadDeleteTaskClick,
/// mangadownloader/forms/frmMain.pas:2214-2310).
#[utoipa::path(delete, path = "/api/tasks/{id}", tag = "tasks", operation_id = "deleteTask",
    params(("id" = i64, Path, description = "Task ID"), DeleteQuery),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, body = Problem, description = "No such task"),
    ))]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiQuery(query): ApiQuery<DeleteQuery>,
) -> Result<StatusCode, ApiError> {
    state.engine.delete(TaskId(id), query.files).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Which tasks to remove in bulk.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct RemoveQuery {
    /// Must be `finished`.
    status: Option<TaskGroup>,
}

/// Remove the finished tasks, keeping their files (`RemoveAllFinishedTasks`,
/// baseunits/uDownloadsManager.pas:1987-2001).
#[utoipa::path(delete, path = "/api/tasks", tag = "tasks", operation_id = "removeFinishedTasks",
    params(RemoveQuery),
    responses(
        (status = 204, description = "Removed"),
        (status = 400, body = Problem, description = "`status` is not `finished`"),
    ))]
pub(crate) async fn remove_finished(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<RemoveQuery>,
) -> Result<StatusCode, ApiError> {
    if query.status != Some(TaskGroup::Finished) {
        return Err(ApiError::BadRequest(
            "only finished tasks can be removed in bulk: pass status=finished".into(),
        ));
    }
    state.engine.remove_finished().await?;
    Ok(StatusCode::NO_CONTENT)
}
