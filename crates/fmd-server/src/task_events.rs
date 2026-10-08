//! The download engine's events as `/api/events` frames: what FMD2's downloads view shows on
//! each refresh (`tmRefreshDownloadsInfoTimer`, mangadownloader/forms/frmMain.pas:2015-2028).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fmd_core::download::{EngineEvent, Progress, TaskId, TaskStatus};
use futures_util::stream::{self, BoxStream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::events::{ServerEvent, TaskProgress, TaskRemoved, TaskStatusChange};
use crate::services::DownloadEngine;
use crate::tasks::chapter_label;

/// The shortest time between two `task.progress` frames of one task: at most 4 a second.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// `engine`'s events from now on as server events. Subscribes right away, so nothing the engine
/// sends after this returns is missed.
pub(crate) fn stream(engine: Arc<dyn DownloadEngine>) -> BoxStream<'static, ServerEvent> {
    let Some(rx) = engine.subscribe() else {
        return stream::empty().boxed();
    };
    let feed = Feed {
        engine,
        tasks: HashMap::new(),
    };
    stream::unfold(
        (BroadcastStream::new(rx), feed),
        |(mut rx, mut feed)| async move {
            loop {
                // A lagging subscriber skips what it missed; the next frames catch it up.
                let Ok(event) = rx.next().await? else {
                    continue;
                };
                if let Some(event) = feed.convert(event).await {
                    return Some((event, (rx, feed)));
                }
            }
        },
    )
    .boxed()
}

/// What a frame needs to know about a task beyond the engine event.
struct Known {
    title: String,
    chapters: Vec<String>,
    status: TaskStatus,
    /// When the last `task.progress` frame went out.
    progress_at: Option<Instant>,
}

struct Feed {
    engine: Arc<dyn DownloadEngine>,
    tasks: HashMap<TaskId, Known>,
}

impl Feed {
    async fn convert(&mut self, event: EngineEvent) -> Option<ServerEvent> {
        match event {
            EngineEvent::Status {
                task,
                status,
                error,
                ..
            } => {
                if let Some(known) = self.tasks.get_mut(&task) {
                    known.status = status;
                }
                Some(ServerEvent::TaskStatus(TaskStatusChange {
                    id: task.0,
                    status: status.into(),
                    error,
                }))
            }
            EngineEvent::Progress(progress) => self.progress(progress).await,
            EngineEvent::Deleted { task } => {
                self.tasks.remove(&task);
                Some(ServerEvent::TaskRemoved(TaskRemoved { id: task.0 }))
            }
            EngineEvent::Reordered => Some(ServerEvent::TasksReordered),
            // A status change follows each of these when the task's status changes.
            EngineEvent::Added { .. }
            | EngineEvent::Chapter { .. }
            | EngineEvent::Enabled { .. } => None,
        }
    }

    async fn progress(&mut self, progress: Progress) -> Option<ServerEvent> {
        if !self.tasks.contains_key(&progress.task) {
            let info = self
                .engine
                .list()
                .await
                .ok()?
                .into_iter()
                .find(|info| info.task.id == progress.task)?;
            self.tasks.insert(
                progress.task,
                Known {
                    title: info.task.title,
                    chapters: info.chapters.into_iter().map(|c| c.name).collect(),
                    status: info.task.status,
                    progress_at: None,
                },
            );
        }
        let known = self.tasks.get_mut(&progress.task)?;
        let now = Instant::now();
        let last_page = progress.pages_done >= progress.pages_total;
        if !last_page
            && known
                .progress_at
                .is_some_and(|at| now.duration_since(at) < PROGRESS_INTERVAL)
        {
            return None;
        }
        known.progress_at = Some(now);
        let names: Vec<&str> = known.chapters.iter().map(String::as_str).collect();
        Some(ServerEvent::TaskProgress(TaskProgress {
            id: progress.task.0,
            title: known.title.clone(),
            chapters: chapter_label(&names, progress.chapter),
            status: known.status.into(),
            done: u64::from(progress.pages_done),
            total: u64::from(progress.pages_total),
            bytes_per_sec: progress.bytes_per_sec as f64,
        }))
    }
}
