//! Pushing inbox events and failures to the notification channels (`notifications` settings),
//! from one place: the event bus and the download engine's events.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use fmd_core::download::{EngineEvent, TaskId, TaskInfo, TaskStatus};
use fmd_core::favorites::FavoritesEventKind;
use fmd_core::notifications::{
    Channel, Notification, SendError, Sender, chapter_ranges, request, requests,
};
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
/// The kind of a queued download, which has no inbox event.
const DOWNLOAD_QUEUED: &str = "download_queued";
/// The kind of the inbox events saying downloads are held for a full disk, and resumed.
const DISK_SPACE_LOW: &str = "disk_space_low";
/// The kind reported for a favorites check that failed, which has no inbox event.
const FAVORITES_CHECK_FAILED: &str = "favorites_check_failed";
/// The kind of a "Send test" notification.
const TEST: &str = "test";

impl AppState {
    /// Adds each download that fails, and downloads held for a full disk and resumed, to the
    /// inbox, and sends every new inbox item, finished download and failed favorites check to the
    /// notification channels that take it. Sending never holds up the work that caused it: each
    /// request runs on its own, and a failure is only logged.
    pub fn start_notifications(&self) {
        let sender = match Sender::new(SEND_TIMEOUT) {
            Ok(sender) => sender,
            Err(e) => {
                tracing::error!(target: "fmd_server", "notifications are off: {e}");
                return;
            }
        };
        if self.notifier.set(sender).is_err() {
            tracing::warn!(target: "fmd_server", "notifications were already started");
            return;
        }
        // Subscribed before returning, so nothing published after this is missed.
        let bus = self.events.subscribe();
        if let Some(engine) = self.engine.subscribe() {
            tokio::spawn(report_downloads(self.clone(), engine));
        }
        tokio::spawn(forward(self.clone(), bus));
    }

    /// Sends `notification` to each channel that takes it, each request on its own; nothing
    /// without [`AppState::start_notifications`].
    pub(crate) fn push_notification(&self, notification: &Notification) {
        let Some(sender) = self.notifier.get() else {
            return;
        };
        for (channel, request) in requests(&self.settings.get(), notification) {
            let sender = sender.clone();
            tokio::spawn(async move {
                if let Err(e) = sender.send(&request).await {
                    warn_unsent(channel, &e);
                }
            });
        }
    }
}

/// Sends what [`notification`] makes of each bus event.
async fn forward(state: AppState, mut bus: tokio::sync::broadcast::Receiver<ServerEvent>) {
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
            state.push_notification(&notification);
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
            fields: Vec::new(),
            cover_url: None,
            link: None,
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
        fields: Vec::new(),
        cover_url: None,
        link: None,
    }
}

/// Adds an error to the inbox for each task that fails, and an item when downloads are held for
/// a full disk and when they resume, which [`forward`] then sends on; sends each task that is
/// queued or finishes straight to the channels that take it.
async fn report_downloads(
    state: AppState,
    mut engine: tokio::sync::broadcast::Receiver<EngineEvent>,
) {
    loop {
        let event = match engine.recv().await {
            Ok(EngineEvent::Added { task }) => {
                let info = task_info(&state, task).await;
                let notification = download(&state, Sent::Queued, task, info.as_ref()).await;
                state.push_notification(&notification);
                continue;
            }
            Ok(EngineEvent::Status {
                task,
                status: status @ (TaskStatus::Failed | TaskStatus::Finished),
                error,
                ..
            }) => {
                let info = task_info(&state, task).await;
                if status == TaskStatus::Finished {
                    let notification = download(&state, Sent::Completed, task, info.as_ref()).await;
                    state.push_notification(&notification);
                    continue;
                }
                failed_download(task, info, error)
            }
            Ok(EngineEvent::DiskSpaceLow { folder, free_bytes }) => {
                downloads_on_hold(&folder, free_bytes)
            }
            Ok(EngineEvent::DiskSpaceRecovered) => downloads_resumed(),
            Ok(_) => continue,
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(target: "fmd_server",
                    "{missed} download events were skipped; a failed download or a full disk may be missing from the inbox, or a queued or finished one unsent");
                continue;
            }
            Err(RecvError::Closed) => return,
        };
        if let Err(e) = state.notify(event).await {
            tracing::warn!(target: "fmd_server", "adding a download event to the inbox: {e}");
        }
    }
}

/// Downloads are held: `folder`'s disk has only `free_bytes` free.
fn downloads_on_hold(folder: &str, free_bytes: u64) -> NewEvent {
    NewEvent {
        kind: DISK_SPACE_LOW.into(),
        severity: EventSeverity::Warning,
        module_id: None,
        task_id: None,
        title: "Downloads are on hold: the disk is almost full".into(),
        body: serde_json::Value::String(format!(
            "{folder} has {} MB free. Downloads carry on by themselves once there is room again.",
            free_bytes / (1024 * 1024)
        )),
    }
}

fn downloads_resumed() -> NewEvent {
    NewEvent {
        kind: DISK_SPACE_LOW.into(),
        severity: EventSeverity::Info,
        module_id: None,
        task_id: None,
        title: "Downloads resumed".into(),
        body: serde_json::Value::String("The disk has room again.".into()),
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

/// What happened to a download that is sent without an inbox event.
#[derive(Clone, Copy)]
enum Sent {
    Queued,
    Completed,
}

/// Names the task's chapters as ranges and its website, with the series' cover when it is in
/// the library.
async fn download(
    state: &AppState,
    sent: Sent,
    task: TaskId,
    info: Option<&TaskInfo>,
) -> Notification {
    let (kind, what) = match sent {
        Sent::Queued => (DOWNLOAD_QUEUED, "Download queued"),
        Sent::Completed => (DOWNLOAD_COMPLETED, "Download completed"),
    };
    let (fields, cover_url) = match info {
        Some(info) => {
            let names: Vec<&str> = info.chapters.iter().map(|c| c.name.as_str()).collect();
            let fields = vec![
                ("Chapters".to_owned(), chapter_ranges(&names)),
                (
                    "Website".to_owned(),
                    website_name(state, &info.task.module_id),
                ),
            ];
            (
                fields,
                library_cover(state, &info.task.module_id, &info.task.link).await,
            )
        }
        None => (Vec::new(), None),
    };
    Notification {
        kind: kind.into(),
        severity: EventSeverity::Info,
        title: format!("{what}: {}", task_title(task, info)),
        body: String::new(),
        created_at: crate::time::now_rfc3339(),
        fields,
        cover_url,
        link: Some("/queue".into()),
    }
}

/// The cover of the library's series `link` on module `module_id`; `None` when it is not in the
/// library or has no cover.
async fn library_cover(state: &AppState, module_id: &str, link: &str) -> Option<String> {
    let (module_id, link) = (module_id.to_owned(), link.to_owned());
    state
        .blocking(move |db| db.favorites().find(&module_id, &link))
        .await
        .ok()
        .flatten()
        .and_then(|favorite| favorite.cover_url)
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
        fields: Vec::new(),
        cover_url: None,
        link: None,
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
