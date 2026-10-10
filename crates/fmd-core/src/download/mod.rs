//! The download engine, mirroring FMD2's `TDownloadManager`, `TTaskThread` and
//! `TDownloadThread` (baseunits/uDownloadsManager.pas).
//!
//! # Threads
//!
//! Like FMD2, each running task is an OS thread (`TTaskThread`) with page threads
//! (`TDownloadThread`); callbacks run on the [`WorkerPool`]. No Lua or HTTP work runs on a tokio
//! thread.
//!
//! # Status transitions
//!
//! | From | To | When | FMD2 |
//! |---|---|---|---|
//! | (new) | Waiting, or Stopped when `add_as_stopped` | [`DownloadManager::add_task`] | mangadownloader/forms/frmMain.pas:2754-2763 |
//! | Stopped, Failed | Waiting | `start`, `start_all` | `SetTaskActive`, `StartAllTasks` (baseunits/uDownloadsManager.pas:1835-1844, :1922-1941) |
//! | any but Waiting, while not running | Waiting, all chapters pending | `redownload` | `RedownloadTask` (:1846-1857) |
//! | Waiting | running (Preparing) | a free slot: under `max_parallel_tasks` and the module's `CanCreateTask` | `CheckAndActiveTask` (:1784-1833), baseunits/WebsiteModules.pas:414-420 |
//! | Preparing | Downloading | page count and page links known | `TTaskThread.Execute` (:1180-1268) |
//! | Downloading | Converting, then Compressing | every page saved | :1276-1294 |
//! | Downloading, Converting, Compressing | Failed (the task carries on with the next chapter) | a page missing, conversion or packing failed | :1280-1310 |
//! | Compressing, Failed | Preparing | next chapter, or a failed one retried | :1131-1160, :1324-1338 |
//! | last chapter done | Finished, or Failed when a chapter failed | | :1346-1362 |
//! | Waiting | Stopped | `stop`, `stop_all` | `StopTask` (:1900-1920) |
//! | running | Stopped | `stop`, `stop_all`: the thread is terminated | `TTaskThread.Destroy` (:485-528) |
//! | any | Disabled / Stopped | `disable` / `enable` | `TTaskContainer.SetEnabled` (:1384-1397) |
//! | Downloading, Preparing, Waiting (and Converting, Compressing: a killed process) at startup | running, or Waiting; Stopped when the module is gone | [`DownloadManager::open`] | `CheckAndActiveTaskAtStartup` (:1859-1893) |
//!
//! A task running when the manager is dropped keeps its status, so the next
//! [`DownloadManager::open`] resumes it (`StopAllDownloadTasksForExit`, :1957-1977).

mod files;
mod manager;
mod page;
mod preview;
mod task;

use std::sync::Arc;

use fmd_http::HttpClient;
use fmd_lua::{Module, ModuleRegistry, WorkerPool};
use fmd_store::{AppDb, StoreError};
use tokio::sync::broadcast;

use crate::settings::{SettingsError, SettingsService};
use manager::Inner;
pub(crate) use manager::rename_options;
pub use manager::save_to;

pub use fmd_store::{ChapterStatus, Task, TaskChapter, TaskId, TaskStatus};
pub use preview::{PagePlacement, SampleChapter, first_page};

pub type ModuleLookup = dyn Fn(&str) -> Option<Arc<Module>> + Send + Sync;

#[derive(Clone)]
pub struct EngineConfig {
    pub db: AppDb,
    pub pool: Arc<WorkerPool>,
    pub modules: Arc<ModuleLookup>,
    pub settings: Arc<SettingsService>,
    /// For page `GET`s when a module has no `OnDownloadImage`.
    pub http: HttpClient,
}

impl EngineConfig {
    pub fn new(
        db: AppDb,
        pool: Arc<WorkerPool>,
        registry: Arc<ModuleRegistry>,
        settings: Arc<SettingsService>,
        http: HttpClient,
    ) -> EngineConfig {
        EngineConfig {
            db,
            pool,
            modules: Arc::new(move |id| registry.get(id).cloned()),
            settings,
            http,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewDownload {
    pub module_id: String,
    /// As given to `OnGetInfo`.
    pub manga_link: String,
    pub title: String,
    pub authors: String,
    pub artists: String,
    pub chapters: Vec<ChapterSpec>,
    /// Parent of the manga's folder; the default destination when empty.
    pub save_to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChapterSpec {
    pub link: String,
    pub title: String,
    /// Position in the chapter list, for `%NUMBERING%`.
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    Added {
        task: TaskId,
    },
    /// `chapter` is the chapter the task is at.
    Status {
        task: TaskId,
        status: TaskStatus,
        chapter: u32,
        error: Option<String>,
    },
    Chapter {
        task: TaskId,
        chapter: u32,
        status: ChapterStatus,
    },
    /// At most a few times a second.
    Progress(Progress),
    Enabled {
        task: TaskId,
        enabled: bool,
    },
    Deleted {
        task: TaskId,
    },
    Reordered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub task: TaskId,
    pub chapter: u32,
    /// Pages done in the current phase, FMD2's `DownCounter`.
    pub pages_done: u32,
    pub pages_total: u32,
    /// Since the task started.
    pub bytes: u64,
    /// Over the last report interval.
    pub bytes_per_sec: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInfo {
    pub task: Task,
    pub chapters: Vec<TaskChapter>,
    pub running: bool,
    pub progress: Option<Progress>,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("settings: {0}")]
    Settings(#[from] SettingsError),
    #[error("no task {0:?}")]
    NoTask(TaskId),
    #[error("no module {0}")]
    NoModule(String),
    #[error("a task needs at least one chapter")]
    NoChapters,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("engine thread: {0}")]
    Thread(String),
}

/// The download queue. Dropping it terminates running tasks but keeps their status, so
/// [`DownloadManager::open`] resumes them.
pub struct DownloadManager {
    inner: Arc<Inner>,
}

impl DownloadManager {
    /// Loads the queue from `app.db` and resumes the tasks that were running or waiting
    /// (`Restore` and `CheckAndActiveTaskAtStartup`, baseunits/uDownloadsManager.pas:1638-1691,
    /// :1859-1893).
    pub async fn open(config: EngineConfig) -> Result<DownloadManager, EngineError> {
        let inner = Arc::new(Inner::new(config));
        let manager = DownloadManager { inner };
        manager
            .blocking(Inner::check_and_active_task_at_startup)
            .await?;
        Ok(manager)
    }

    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Arc<Inner>) -> Result<T, EngineError> + Send + 'static,
    ) -> Result<T, EngineError> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || f(&inner))
            .await
            .map_err(|e| EngineError::Thread(e.to_string()))?
    }

    /// Queues at the end and starts it when a slot is free.
    pub async fn add_task(&self, download: NewDownload) -> Result<TaskId, EngineError> {
        self.blocking(move |inner| inner.add_task(&download)).await
    }

    /// Sets a stopped or failed task waiting, then starts what can start (`SetTaskActive`,
    /// baseunits/uDownloadsManager.pas:1835-1844).
    pub async fn start(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.start(id)).await
    }

    /// `StopTask` (baseunits/uDownloadsManager.pas:1900-1920). A running task becomes Stopped
    /// once its thread has ended.
    pub async fn stop(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.stop(id)).await
    }

    /// `CheckAndActiveTask` (baseunits/uDownloadsManager.pas:1784-1833), e.g. after an import
    /// queued tasks straight into `app.db`.
    pub async fn activate_waiting(&self) -> Result<(), EngineError> {
        self.blocking(Inner::check_and_active_task).await
    }

    /// `StartAllTasks` (baseunits/uDownloadsManager.pas:1922-1941).
    pub async fn start_all(&self) -> Result<(), EngineError> {
        self.blocking(Inner::start_all).await
    }

    /// `StopAllTasks` (baseunits/uDownloadsManager.pas:1943-1955).
    pub async fn stop_all(&self) -> Result<(), EngineError> {
        self.blocking(Inner::stop_all).await
    }

    /// `Delete` (baseunits/uDownloadsManager.pas:1979-1985); `delete_files` also removes the
    /// chapters' folders and archives.
    pub async fn delete(&self, id: TaskId, delete_files: bool) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.delete(id, delete_files))
            .await
    }

    /// The task becomes Stopped.
    pub async fn enable(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.set_enabled(id, true))
            .await
    }

    /// Start and start-all skip it until it is enabled.
    pub async fn disable(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.set_enabled(id, false))
            .await
    }

    /// `RedownloadTask` (baseunits/uDownloadsManager.pas:1846-1857). Pages and archives on
    /// disk are still skipped.
    pub async fn redownload(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.redownload(id)).await
    }

    /// Puts `ids` first in the queue, in that order.
    pub async fn reorder(&self, ids: Vec<TaskId>) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.reorder(&ids)).await
    }

    pub async fn list(&self) -> Result<Vec<TaskInfo>, EngineError> {
        self.blocking(Inner::list).await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.subscribe()
    }

    /// Like dropping the manager, but waits on a blocking thread instead of a tokio worker.
    pub async fn shutdown(self) -> Result<(), EngineError> {
        self.blocking(|inner| {
            inner.shutdown();
            Ok(())
        })
        .await
    }
}

impl Drop for DownloadManager {
    fn drop(&mut self) {
        self.inner.shutdown();
    }
}
