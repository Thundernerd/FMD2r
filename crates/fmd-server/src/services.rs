//! Services later tickets plug into [`crate::AppState`], kept behind traits so the server does
//! not drive the Lua runtime or the download engine itself; it only sees `fmd-core`'s view of
//! them.

use fmd_core::info::{InfoError, InfoOptions, MangaInfo};
use fmd_core::modules::ModuleInfo;
use futures_util::future::BoxFuture;
use serde::Serialize;
use utoipa::ToSchema;

use crate::covers::{CoverModules, CoverSession};

/// The download engine (T20). Queue endpoints (T23) add the methods they need.
pub trait DownloadEngine: Send + Sync + 'static {}

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

/// Stand-in until the real services exist: no tasks, no modules (so no covers).
pub struct Idle;

impl DownloadEngine for Idle {}

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
