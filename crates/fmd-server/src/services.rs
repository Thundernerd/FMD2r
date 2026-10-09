//! Services later tickets plug into [`crate::AppState`], kept behind traits so the server does
//! not drive the Lua runtime or the download engine itself; it only sees `fmd-core`'s view of
//! them.

use fmd_core::download::{
    DownloadManager, EngineError, EngineEvent, NewDownload, TaskId, TaskInfo, TaskStatus,
};
use fmd_core::favorites::{CheckError, CheckMode, CheckScope, FavoritesChecker};
use fmd_core::info::{InfoError, InfoOptions, MangaInfo};
use fmd_core::modules::ModuleInfo;
use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::sync::broadcast;
use utoipa::ToSchema;

use crate::covers::{CoverModules, CoverSession};

/// The download engine (`fmd_core::download::DownloadManager`, T20) as the queue endpoints
/// drive it: FMD2's `TDownloadManager` operations (baseunits/uDownloadsManager.pas:1769-2045).
pub trait DownloadEngine: Send + Sync + 'static {
    /// Every task in queue order.
    fn list(&self) -> BoxFuture<'_, Result<Vec<TaskInfo>, EngineError>>;

    /// Queues `download` at the end of the queue (`btDownloadClick`,
    /// mangadownloader/forms/frmMain.pas:2646-2795).
    fn add(&self, download: NewDownload) -> BoxFuture<'_, Result<TaskId, EngineError>>;

    /// Sets a stopped or failed task waiting (`SetTaskActive`,
    /// baseunits/uDownloadsManager.pas:1835-1844).
    fn start(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>>;

    /// `StopTask` (baseunits/uDownloadsManager.pas:1900-1920).
    fn stop(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>>;

    /// `RedownloadTask` (baseunits/uDownloadsManager.pas:1846-1857).
    fn redownload(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>>;

    /// `EnableTask` (baseunits/uDownloadsManager.pas:2022-2026).
    fn enable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>>;

    /// `DisableTask` (baseunits/uDownloadsManager.pas:2028-2045).
    fn disable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>>;

    /// `StartAllTasks` (baseunits/uDownloadsManager.pas:1922-1941).
    fn start_all(&self) -> BoxFuture<'_, Result<(), EngineError>>;

    /// `StopAllTasks` (baseunits/uDownloadsManager.pas:1943-1955).
    fn stop_all(&self) -> BoxFuture<'_, Result<(), EngineError>>;

    /// Removes a task, stopping it first; with `files`, also its chapters' folders and archives
    /// (`Delete`, baseunits/uDownloadsManager.pas:1979-1985).
    fn delete(&self, id: TaskId, files: bool) -> BoxFuture<'_, Result<(), EngineError>>;

    /// Puts `ids` first in the queue, in that order.
    fn reorder(&self, ids: Vec<TaskId>) -> BoxFuture<'_, Result<(), EngineError>>;

    /// Starts waiting tasks while there are free slots (`CheckAndActiveTask`,
    /// baseunits/uDownloadsManager.pas:1784-1833), e.g. the ones an import queued.
    fn activate_waiting(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// The engine's events from now on; `None` when it has none.
    fn subscribe(&self) -> Option<broadcast::Receiver<EngineEvent>> {
        None
    }

    /// `RemoveAllFinishedTasks` (baseunits/uDownloadsManager.pas:1987-2001): deletes the
    /// finished tasks, keeping their files.
    fn remove_finished(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(async move {
            for info in self.list().await? {
                if info.task.status == TaskStatus::Finished {
                    self.delete(info.task.id, false).await?;
                }
            }
            Ok(())
        })
    }
}

impl DownloadEngine for DownloadManager {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TaskInfo>, EngineError>> {
        Box::pin(DownloadManager::list(self))
    }

    fn add(&self, download: NewDownload) -> BoxFuture<'_, Result<TaskId, EngineError>> {
        Box::pin(self.add_task(download))
    }

    fn start(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::start(self, id))
    }

    fn stop(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::stop(self, id))
    }

    fn redownload(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::redownload(self, id))
    }

    fn enable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::enable(self, id))
    }

    fn disable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::disable(self, id))
    }

    fn activate_waiting(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::activate_waiting(self))
    }

    fn start_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::start_all(self))
    }

    fn stop_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::stop_all(self))
    }

    fn delete(&self, id: TaskId, files: bool) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::delete(self, id, files))
    }

    fn reorder(&self, ids: Vec<TaskId>) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(DownloadManager::reorder(self, ids))
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<EngineEvent>> {
        Some(DownloadManager::subscribe(self))
    }
}

/// The loaded website modules (T06, T14) and their upstream sync state (T29), for
/// `GET /api/about` and the per-module settings.
pub trait ModuleCatalog: Send + Sync + 'static {
    /// Must be cheap: it is called on the async threads.
    fn report(&self) -> ModulesReport;

    /// Every loaded module, sorted by ID. Must be cheap: it is called on the async threads.
    fn modules(&self) -> Vec<ModuleInfo> {
        Vec::new()
    }

    /// The loaded module with ID `id`.
    fn module(&self, id: &str) -> Option<ModuleInfo> {
        self.modules().into_iter().find(|m| m.id == id)
    }

    /// The info of the series at `link` (relative to the module's `RootURL`) from module `id`'s
    /// `OnGetInfo`, cleaned up with `options` (`fmd_core::info::get_info`).
    fn get_info(
        &self,
        id: &str,
        link: &str,
        options: InfoOptions,
    ) -> BoxFuture<'static, Result<MangaInfo, InfoError>> {
        let _ = (id, link, options);
        Box::pin(std::future::ready(Err(InfoError::UnknownModule)))
    }
}

/// The favorites check (`fmd_core::favorites::FavoritesChecker`), for `POST /api/favorites/check`
/// and `POST /api/favorites/{id}/check-missing`.
pub trait FavoritesJobs: Send + Sync + 'static {
    /// Starts checking the favorites in `scope` for `mode` chapters and returns without waiting;
    /// fails when a check is running.
    fn check(&self, scope: CheckScope, mode: CheckMode) -> Result<(), CheckError>;
}

impl FavoritesJobs for FavoritesChecker {
    fn check(&self, scope: CheckScope, mode: CheckMode) -> Result<(), CheckError> {
        self.start(scope, mode)
    }
}

/// What [`ModuleCatalog::report`] knows about the Lua modules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModulesReport {
    /// The upstream ref the Lua tree follows (e.g. `master`).
    pub upstream_ref: Option<String>,
    /// The upstream commit the Lua tree was last synced to.
    pub upstream_sha: Option<String>,
    pub module_count: u64,
    /// The XPath engine behind `CreateTXQuery` (e.g. `fpc`).
    pub xpath_backend: Option<String>,
    pub load_failures: Vec<LoadFailure>,
}

/// A module file that failed to load (`Init` error or unknown Host API).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct LoadFailure {
    pub module: String,
    pub error: String,
    /// The inbox item reporting it.
    pub inbox_id: Option<String>,
}

/// Stand-in until the real services exist: no tasks (and no modules to queue one with), no
/// modules (so no covers).
pub struct Idle;

impl DownloadEngine for Idle {
    fn list(&self) -> BoxFuture<'_, Result<Vec<TaskInfo>, EngineError>> {
        ready(Ok(Vec::new()))
    }

    fn add(&self, download: NewDownload) -> BoxFuture<'_, Result<TaskId, EngineError>> {
        ready(Err(EngineError::NoModule(download.module_id)))
    }

    fn start(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }

    fn stop(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }

    fn redownload(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }

    fn enable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }

    fn disable(&self, id: TaskId) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }

    fn start_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Ok(()))
    }

    fn stop_all(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Ok(()))
    }

    fn delete(&self, id: TaskId, _files: bool) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Err(EngineError::NoTask(id)))
    }

    fn reorder(&self, _ids: Vec<TaskId>) -> BoxFuture<'_, Result<(), EngineError>> {
        ready(Ok(()))
    }
}

fn ready<T: Send + 'static>(value: T) -> BoxFuture<'static, T> {
    Box::pin(std::future::ready(value))
}

impl ModuleCatalog for Idle {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }
}

impl CoverModules for Idle {
    fn cover_session(&self, _id: &str) -> Option<CoverSession> {
        None
    }
}
