//! The download engine, mirroring FMD2's `TDownloadManager`, `TTaskThread` and
//! `TDownloadThread` (baseunits/uDownloadsManager.pas).
//!
//! # Threads
//!
//! Like FMD2, every running task is a dedicated OS thread (`TTaskThread`) that runs the
//! per-chapter pipeline, and downloads its pages on further OS threads (`TDownloadThread`).
//! Module callbacks run on the [`WorkerPool`]'s Lua threads; page `GET`s run on the page
//! threads with a blocking [`fmd_http::HttpSession`]. No Lua or HTTP work runs on a tokio
//! thread: the async [`DownloadManager`] methods only touch `app.db`, inside
//! `spawn_blocking`.
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
//! A task still running when the manager is dropped keeps its status, so the next
//! [`DownloadManager::open`] resumes it (`StopAllDownloadTasksForExit`, :1957-1977, and
//! `isReadyForExit` in :494).

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
pub(crate) use manager::{rename_options, save_to};

pub use fmd_store::{ChapterStatus, Task, TaskChapter, TaskId, TaskStatus};
pub use preview::{PagePlacement, SampleChapter, first_page};

/// Finds a loaded module by ID.
pub type ModuleLookup = dyn Fn(&str) -> Option<Arc<Module>> + Send + Sync;

/// What the engine runs on.
#[derive(Clone)]
pub struct EngineConfig {
    pub db: AppDb,
    /// Runs the module callbacks.
    pub pool: Arc<WorkerPool>,
    pub modules: Arc<ModuleLookup>,
    pub settings: Arc<SettingsService>,
    /// The client the page `GET`s are made with when a module has no `OnDownloadImage`.
    pub http: HttpClient,
}

impl EngineConfig {
    /// An engine over the modules of `registry`.
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

/// A download to queue: chapters of one manga.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewDownload {
    pub module_id: String,
    /// The manga's link, as the module's `OnGetInfo` was given it.
    pub manga_link: String,
    pub title: String,
    pub authors: String,
    pub artists: String,
    pub chapters: Vec<ChapterSpec>,
    /// The directory the manga's folder is made in; the configured download directory when
    /// empty.
    pub save_to: String,
}

/// One chapter of a [`NewDownload`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChapterSpec {
    pub link: String,
    pub title: String,
    /// The chapter's position in the manga's chapter list, for `%NUMBERING%`.
    pub number: u32,
}

/// What the engine reports, for the event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    Added {
        task: TaskId,
    },
    /// The task's status changed; `chapter` is the chapter it is at.
    Status {
        task: TaskId,
        status: TaskStatus,
        chapter: u32,
        error: Option<String>,
    },
    /// A chapter was downloaded or failed.
    Chapter {
        task: TaskId,
        chapter: u32,
        status: ChapterStatus,
    },
    /// Progress of the chapter being downloaded, at most a few times a second.
    Progress(Progress),
    Enabled {
        task: TaskId,
        enabled: bool,
    },
    Deleted {
        task: TaskId,
    },
    /// The queue order changed.
    Reordered,
}

/// How far a running task's current chapter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub task: TaskId,
    pub chapter: u32,
    /// Pages done of the current phase (page links or images), FMD2's `DownCounter`.
    pub pages_done: u32,
    pub pages_total: u32,
    /// Bytes downloaded by the task since it started.
    pub bytes: u64,
    /// Download speed over the last report interval.
    pub bytes_per_sec: u64,
}

/// A task as the queue lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInfo {
    pub task: Task,
    pub chapters: Vec<TaskChapter>,
    pub running: bool,
    /// The last progress of a running task.
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

/// The download queue. Tasks run on their own threads; dropping the manager terminates them
/// and waits for them, leaving their status as it was so [`DownloadManager::open`] resumes
/// them.
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

    /// Queues `download` as a new task at the end of the queue and starts it when a slot is
    /// free.
    pub async fn add_task(&self, download: NewDownload) -> Result<TaskId, EngineError> {
        self.blocking(move |inner| inner.add_task(&download)).await
    }

    /// Sets a stopped or failed task waiting, then starts what can start (`SetTaskActive`,
    /// baseunits/uDownloadsManager.pas:1835-1844).
    pub async fn start(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.start(id)).await
    }

    /// Stops a waiting or running task (`StopTask`, baseunits/uDownloadsManager.pas:1900-1920).
    /// A running task becomes Stopped once its thread has ended, which terminating its HTTP
    /// requests and Lua waits makes prompt.
    pub async fn stop(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.stop(id)).await
    }

    /// `CheckAndActiveTask` (baseunits/uDownloadsManager.pas:1784-1833): starts waiting tasks
    /// while there are free slots, e.g. tasks queued straight into `app.db` by an import.
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

    /// Removes a task, stopping it first; with `delete_files`, also its chapters' folders and
    /// archives (`Delete`, baseunits/uDownloadsManager.pas:1979-1985).
    pub async fn delete(&self, id: TaskId, delete_files: bool) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.delete(id, delete_files))
            .await
    }

    /// Re-enables a disabled task, which becomes Stopped.
    pub async fn enable(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.set_enabled(id, true))
            .await
    }

    /// Stops a task and sets it Disabled; start and start-all skip it until it is enabled.
    pub async fn disable(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.set_enabled(id, false))
            .await
    }

    /// Downloads every chapter of a task again (`RedownloadTask`,
    /// baseunits/uDownloadsManager.pas:1846-1857). Pages and archives already on disk are
    /// still skipped.
    pub async fn redownload(&self, id: TaskId) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.redownload(id)).await
    }

    /// Puts `ids` first in the queue, in that order; waiting tasks start in queue order.
    pub async fn reorder(&self, ids: Vec<TaskId>) -> Result<(), EngineError> {
        self.blocking(move |inner| inner.reorder(&ids)).await
    }

    /// Every task in queue order.
    pub async fn list(&self) -> Result<Vec<TaskInfo>, EngineError> {
        self.blocking(Inner::list).await
    }

    /// The engine's events from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.subscribe()
    }

    /// Terminates every running task and waits for its thread on a blocking thread, leaving
    /// statuses as they are, so the next [`DownloadManager::open`] resumes them. Dropping the
    /// manager does the same but waits on the dropping thread, which blocks a tokio worker.
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
