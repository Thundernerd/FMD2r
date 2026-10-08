//! Services later tickets plug into [`crate::AppState`], kept behind traits so the server does
//! not depend on the engine crates.

use serde::Serialize;
use utoipa::ToSchema;

/// The download engine (T20). Queue endpoints (T23) add the methods they need.
pub trait DownloadEngine: Send + Sync + 'static {}

/// The loaded website modules (T06, T14) and their upstream sync state (T29), for
/// `GET /api/about`.
pub trait ModuleCatalog: Send + Sync + 'static {
    /// Must be cheap: it is called on the async threads.
    fn report(&self) -> ModulesReport;
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

/// Stand-in until the real services exist: no tasks, no modules.
pub struct Idle;

impl DownloadEngine for Idle {}

impl ModuleCatalog for Idle {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }
}
