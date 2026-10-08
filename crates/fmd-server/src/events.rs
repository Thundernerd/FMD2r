//! The event bus and `GET /api/events`, the one SSE stream the UI listens to.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use fmd_core::jobs::{Job, JobPhase};
use fmd_core::lists::{ListEvent, ListEventKind};
use fmd_store::EventId;
use futures_util::stream::{self, Stream, StreamExt};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use utoipa::ToSchema;

use crate::accounts::AccountStateChange;
use crate::inbox::InboxItem;
use crate::logs::LogLine;
use crate::{ApiError, AppState};

/// How many events a slow subscriber may fall behind before it misses some.
const BUS_CAPACITY: usize = 1024;
/// How many stored inbox events a reconnecting client gets replayed at most.
const REPLAY_LIMIT: u32 = 500;
const HEARTBEAT: Duration = Duration::from_secs(15);

/// Task status, mirroring FMD2's `TDownloadStatusType` (baseunits/uDownloadsManager.pas:19-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Stopped,
    Waiting,
    Preparing,
    Downloading,
    Converting,
    Compressing,
    Finished,
    Failed,
    Disabled,
}

impl From<fmd_store::TaskStatus> for TaskState {
    fn from(status: fmd_store::TaskStatus) -> Self {
        use fmd_store::TaskStatus as S;
        match status {
            S::Stopped => Self::Stopped,
            S::Waiting => Self::Waiting,
            S::Preparing => Self::Preparing,
            S::Downloading => Self::Downloading,
            S::Converting => Self::Converting,
            S::Compressing => Self::Compressing,
            S::Finished => Self::Finished,
            S::Failed => Self::Failed,
            S::Disabled => Self::Disabled,
        }
    }
}

/// Download progress of one task (`task.progress`), the data behind FMD2's periodic downloads
/// view refresh (mangadownloader/forms/frmMain.pas:2015).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct TaskProgress {
    pub id: i64,
    pub title: String,
    /// Human readable chapter range, e.g. `Ch. 97–98`.
    pub chapters: String,
    pub status: TaskState,
    /// Pages downloaded.
    pub done: u64,
    /// Pages in total.
    pub total: u64,
    pub bytes_per_sec: f64,
}

/// A task moved to another status (`task.status`), e.g. finished or failed
/// (baseunits/uDownloadsManager.pas:741-803).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct TaskStatusChange {
    pub id: i64,
    pub status: TaskState,
}

/// A background job and its progress (`job.state`, and the items of `GET /api/jobs`): favorites
/// check, list update, module update, or any other registered job.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct JobState {
    pub id: String,
    pub title: String,
    pub state: JobPhase,
    pub done: u64,
    /// 0 when unknown.
    pub total: u64,
    /// RFC 3339 start of the last run.
    pub last_run: Option<String>,
    /// RFC 3339 start of the next scheduled run.
    pub next_run: Option<String>,
    /// Why the last run failed.
    pub last_error: Option<String>,
}

impl JobState {
    /// The current state of `job`.
    pub fn of(job: &dyn Job) -> Self {
        let status = job.status();
        Self {
            id: job.id().to_owned(),
            title: job.title().to_owned(),
            state: status.phase,
            done: status.done,
            total: status.total,
            last_run: status.last_run.map(crate::time::rfc3339_from_unix_ms),
            next_run: status.next_run.map(crate::time::rfc3339_from_unix_ms),
            last_error: status.last_error,
        }
    }
}

/// Everything pushed over `GET /api/events`. The SSE event name is [`ServerEvent::name`]; the
/// data is the payload as JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    TaskProgress(TaskProgress),
    TaskStatus(TaskStatusChange),
    Job(JobState),
    /// A new inbox item; its SSE id is the `events` row id, so clients can resume.
    InboxNew(InboxItem),
    Log(LogLine),
    /// A list update or FMD2-DB import moved on (`job.lists.<kind>`).
    Lists(ListEvent),
    Account(AccountStateChange),
}

impl ServerEvent {
    /// The SSE event name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TaskProgress(_) => "task.progress",
            Self::TaskStatus(_) => "task.status",
            Self::Job(_) => "job.state",
            Self::InboxNew(_) => "inbox.new",
            Self::Log(_) => "log",
            Self::Lists(e) => match e.kind {
                ListEventKind::Started => "job.lists.started",
                ListEventKind::Progress => "job.lists.progress",
                ListEventKind::Finished => "job.lists.finished",
                ListEventKind::Cancelled => "job.lists.cancelled",
                ListEventKind::Failed => "job.lists.failed",
            },
            Self::Account(_) => "account.state",
        }
    }

    fn to_sse(&self) -> Option<SseEvent> {
        let event = SseEvent::default().event(self.name());
        let event = match self {
            Self::TaskProgress(p) => event.json_data(p),
            Self::TaskStatus(s) => event.json_data(s),
            Self::Job(j) => event.json_data(j),
            Self::InboxNew(item) => event.id(item.id.clone()).json_data(item),
            Self::Log(line) => event.json_data(line),
            Self::Lists(e) => event.json_data(e),
            Self::Account(change) => event.json_data(change),
        };
        event.ok()
    }
}

/// Fan-out of [`ServerEvent`]s to every connected SSE client. Cheap to clone.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<ServerEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(BUS_CAPACITY).0,
        }
    }

    /// Sends `event` to every current subscriber; dropped when nobody listens.
    pub fn publish(&self, event: ServerEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.tx.subscribe()
    }
}

/// Server-sent event stream.
#[utoipa::path(get, path = "/api/events", tag = "events", operation_id = "events",
    description = "Named events: `task.progress` (TaskProgress), `task.status` (TaskStatusChange), \
        `job.state` (JobState), `inbox.new` (InboxItem), `log` (LogLine), `account.state` \
        (AccountStateChange), and `job.lists.started|progress|finished|cancelled|failed` \
        (ListEvent). Each frame's data is the JSON payload. `inbox.new` frames carry the inbox \
        item id as the SSE id; on reconnect, \
        `Last-Event-ID` replays the inbox items stored since. Comment frames are heartbeats.",
    params(("Last-Event-ID" = Option<String>, Header, description = "Resume after this inbox item id")),
    responses((status = 200, description = "Event stream", content_type = "text/event-stream")))]
pub(crate) async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    // Subscribe before reading the backlog so nothing published in between is lost.
    let bus = BroadcastStream::new(state.events().subscribe())
        .filter_map(|event| async move { event.ok() });
    let registry = state.jobs.clone();
    let jobs = BroadcastStream::new(registry.subscribe()).filter_map(move |id| {
        let job = id.ok().and_then(|id| registry.get(&id));
        async move { job.map(|job| ServerEvent::Job(JobState::of(job.as_ref()))) }
    });
    let accounts = match &state.accounts {
        Some(accounts) => BroadcastStream::new(accounts.subscribe())
            .filter_map(|change| async move {
                change.ok().map(|c| {
                    ServerEvent::Account(AccountStateChange {
                        module: c.module_id,
                        status: c.status.into(),
                    })
                })
            })
            .boxed(),
        None => stream::empty().boxed(),
    };
    let live = stream::select(stream::select(bus, jobs), accounts);
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<i64>().ok());
    let backlog = match last_id {
        Some(id) => {
            state
                .blocking(move |db| db.events().list_after(EventId(id), REPLAY_LIMIT))
                .await?
        }
        None => Vec::new(),
    };
    // An item stored between subscribing and reading the backlog arrives on both; drop the live copy.
    let replayed_up_to = backlog.last().map_or(i64::MIN, |e| e.id.0);
    let live = live.filter(move |event| {
        let replayed = matches!(event, ServerEvent::InboxNew(item)
            if item.id.parse::<i64>().is_ok_and(|id| id <= replayed_up_to));
        std::future::ready(!replayed)
    });
    let backlog = backlog
        .into_iter()
        .map(|e| ServerEvent::InboxNew(InboxItem::from(e)));
    let events = stream::iter(backlog)
        .chain(live)
        .filter_map(|event| async move { event.to_sse().map(Ok) })
        .take_until(state.shutting_down());
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(HEARTBEAT).text("heartbeat")))
}
