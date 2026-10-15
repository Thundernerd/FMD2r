//! Pushing inbox events and failures to the notification channels (`notifications` settings),
//! from one place: the event bus and the download engine's events.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use fmd_core::download::{EngineEvent, TaskId, TaskInfo, TaskStatus};
use fmd_core::favorites::FavoritesEventKind;
use fmd_core::notifications::{Channel, Notification, SendError, Sender, request, requests};
use fmd_store::{EventSeverity, NewEvent};
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use utoipa::ToSchema;

use crate::error::ApiPath;
use crate::events::ServerEvent;
use crate::inbox::{InboxItem, InboxKind};
use crate::{AppState, Problem};

/// How long a channel gets to accept a notification; there are no retries.
pub(crate) const SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// The kind of a failed download's inbox event.
const DOWNLOAD_FAILED: &str = "download_failed";
/// The kind of a finished download, which has no inbox event.
const DOWNLOAD_COMPLETED: &str = "download_completed";
/// The kind reported for a favorites check that failed, which has no inbox event.
const FAVORITES_CHECK_FAILED: &str = "favorites_check_failed";
/// The kind of a "Send test" notification.
const TEST: &str = "test";

impl AppState {
    /// Adds each download that fails to the inbox, and sends every new inbox item, finished
    /// download and failed favorites check to the notification channels that take it. Sending
    /// never holds up the work that caused it: each request runs on its own, and a failure is
    /// only logged.
    pub fn start_notifications(&self) {
        let sender = match Sender::new(SEND_TIMEOUT) {
            Ok(sender) => sender,
            Err(e) => {
                tracing::error!(target: "fmd_server", "notifications are off: {e}");
                return;
            }
        };
        // Subscribed before returning, so nothing published after this is missed.
        let bus = self.events.subscribe();
        if let Some(engine) = self.engine.subscribe() {
            tokio::spawn(report_downloads(self.clone(), engine, sender.clone()));
        }
        tokio::spawn(forward(self.clone(), bus, sender));
    }
}

/// Sends what [`notification`] makes of each bus event.
async fn forward(
    state: AppState,
    mut bus: tokio::sync::broadcast::Receiver<ServerEvent>,
    sender: Sender,
) {
    loop {
        let event = match bus.recv().await {
            Ok(event) => event,
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(target: "fmd_server", "{missed} notifications were skipped");
                continue;
            }
            Err(RecvError::Closed) => return,
        };
        if let Some(notification) = notification(&event) {
            push(&state, &sender, &notification);
        }
    }
}

fn warn_unsent(channel: Channel, error: &SendError) {
    tracing::warn!(target: "fmd_server",
        "the {} notification was not delivered: {error}", channel.name());
}

/// A new inbox item, or a favorites check that failed.
fn notification(event: &ServerEvent) -> Option<Notification> {
    match event {
        ServerEvent::InboxNew(item) => Some(from_inbox(item)),
        ServerEvent::Favorites(e) if e.kind == FavoritesEventKind::Failed => Some(Notification {
            kind: FAVORITES_CHECK_FAILED.into(),
            severity: EventSeverity::Error,
            title: "Checking the library for new chapters failed".into(),
            body: e.error.clone().unwrap_or_default(),
            created_at: crate::time::now_rfc3339(),
        }),
        _ => None,
    }
}

fn from_inbox(item: &InboxItem) -> Notification {
    Notification {
        kind: item.event_kind.clone(),
        severity: match item.kind {
            InboxKind::Info => EventSeverity::Info,
            InboxKind::Warn => EventSeverity::Warning,
            InboxKind::Error => EventSeverity::Error,
        },
        title: item.title.clone(),
        body: item.body.clone(),
        created_at: item.created_at.clone(),
    }
}

/// Adds an error to the inbox for each task that fails, which [`forward`] then sends on, and
/// sends each task that finishes straight to the channels that take it.
async fn report_downloads(
    state: AppState,
    mut engine: tokio::sync::broadcast::Receiver<EngineEvent>,
    sender: Sender,
) {
    loop {
        let (task, status, error) = match engine.recv().await {
            Ok(EngineEvent::Status {
                task,
                status: status @ (TaskStatus::Failed | TaskStatus::Finished),
                error,
                ..
            }) => (task, status, error),
            Ok(_) => continue,
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(target: "fmd_server",
                    "{missed} download events were skipped; a failed download may be missing from the inbox, or a finished one unsent");
                continue;
            }
            Err(RecvError::Closed) => return,
        };
        let info = task_info(&state, task).await;
        if status == TaskStatus::Finished {
            let website = info
                .as_ref()
                .map(|info| website_name(&state, &info.task.module_id));
            let notification =
                completed_download(task, info.as_ref(), website.as_deref().unwrap_or_default());
            push(&state, &sender, &notification);
            continue;
        }
        let event = failed_download(task, info, error);
        if let Err(e) = state.notify(event).await {
            tracing::warn!(target: "fmd_server", "adding a failed download to the inbox: {e}");
        }
    }
}

/// Sends `notification` to each channel that takes it, each request on its own.
fn push(state: &AppState, sender: &Sender, notification: &Notification) {
    for (channel, request) in requests(&state.settings.get(), notification) {
        let sender = sender.clone();
        tokio::spawn(async move {
            if let Err(e) = sender.send(&request).await {
                warn_unsent(channel, &e);
            }
        });
    }
}

async fn task_info(state: &AppState, task: TaskId) -> Option<TaskInfo> {
    match state.engine.list().await {
        Ok(tasks) => tasks.into_iter().find(|info| info.task.id == task),
        Err(_) => None,
    }
}

/// The name of the website of module `module_id`, or the ID when no module has it.
fn website_name(state: &AppState, module_id: &str) -> String {
    state
        .modules
        .module(module_id)
        .map_or_else(|| module_id.to_owned(), |m| m.name)
}

/// The series title of `task`, or its ID when it could not be found.
fn task_title(task: TaskId, info: Option<&TaskInfo>) -> String {
    info.map_or_else(
        || format!("task {}", task.0),
        |info| info.task.title.clone(),
    )
}

fn failed_download(task: TaskId, info: Option<TaskInfo>, error: Option<String>) -> NewEvent {
    NewEvent {
        kind: DOWNLOAD_FAILED.into(),
        severity: EventSeverity::Error,
        module_id: info.as_ref().map(|info| info.task.module_id.clone()),
        task_id: Some(task),
        title: format!("Download failed: {}", task_title(task, info.as_ref())),
        // A guard: the engine gives every failure a reason.
        body: serde_json::Value::String(error.unwrap_or_else(|| "No reason was given.".into())),
    }
}

/// Lists the task's chapters and `website`, the name of its website.
fn completed_download(task: TaskId, info: Option<&TaskInfo>, website: &str) -> Notification {
    let body = match info {
        Some(info) => {
            let chapters: Vec<&str> = info.chapters.iter().map(|c| c.name.as_str()).collect();
            format!("Chapters: {}\nWebsite: {website}", chapters.join(", "))
        }
        None => String::new(),
    };
    Notification {
        kind: DOWNLOAD_COMPLETED.into(),
        severity: EventSeverity::Info,
        title: format!("Download completed: {}", task_title(task, info)),
        body,
        created_at: crate::time::now_rfc3339(),
    }
}

/// How a test notification went.
#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationTest {
    pub delivered: bool,
    /// Why it was not delivered.
    pub error: Option<String>,
}

/// Send a test notification to a channel with its saved settings, whether or not it is on.
#[utoipa::path(post, path = "/api/notifications/{channel}/test", tag = "settings",
    operation_id = "testNotification",
    params(("channel" = Channel, Path, description = "The channel to send to")),
    responses(
        (status = 200, body = NotificationTest, description = "Whether it was delivered"),
        (status = 400, description = "No such channel", body = Problem),
    ))]
pub(crate) async fn test(
    State(state): State<AppState>,
    ApiPath(channel): ApiPath<Channel>,
) -> Json<NotificationTest> {
    let notification = Notification {
        kind: TEST.into(),
        severity: EventSeverity::Info,
        title: "FMD2r test notification".into(),
        body: "Notifications from FMD2r arrive here.".into(),
        created_at: crate::time::now_rfc3339(),
    };
    let Some(request) = request(channel, &state.settings.get(), &notification) else {
        return Json(NotificationTest {
            delivered: false,
            error: Some("No URL is set.".into()),
        });
    };
    let sent = match Sender::new(SEND_TIMEOUT) {
        Ok(sender) => sender.send(&request).await,
        Err(e) => Err(e),
    };
    Json(NotificationTest {
        delivered: sent.is_ok(),
        error: sent.err().map(|e| e.to_string()),
    })
}
