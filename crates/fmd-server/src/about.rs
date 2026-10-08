//! `GET /api/about`: version, module and storage diagnostics, and tool checks for the System page.

use std::path::Path;

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use crate::services::LoadFailure;
use crate::state::off_thread;
use crate::tools::ToolCheck;
use crate::{ApiError, AppState};

/// The databases [`About::databases`] reports, by file name in the data dir.
const DATABASES: [&str; 2] = ["app.db", "lists.db"];

/// Diagnostics about the running server.
#[derive(Serialize, ToSchema)]
pub(crate) struct About {
    version: &'static str,
    /// The git commit the binary was built from, when known.
    git_revision: Option<&'static str>,
    /// The upstream ref the Lua tree follows (e.g. `master`).
    upstream_ref: Option<String>,
    /// The upstream commit the Lua tree was last synced to.
    upstream_sha: Option<String>,
    /// Website modules loaded.
    module_count: u64,
    /// Module files that failed to load.
    load_failures: Vec<LoadFailure>,
    /// The XPath engine behind `CreateTXQuery`.
    xpath_backend: Option<String>,
    data_dir: Option<String>,
    databases: Vec<DatabaseSize>,
    uptime_secs: u64,
    /// External tools modules and conversions rely on.
    tools: Vec<ToolCheck>,
}

/// The size of one database in the data dir.
#[derive(Serialize, ToSchema)]
pub(crate) struct DatabaseSize {
    name: &'static str,
    /// The database file plus its write-ahead log; `null` when the file does not exist.
    bytes: Option<u64>,
}

/// Server diagnostics. Tool checks run each time, so this may take a few seconds.
#[utoipa::path(get, path = "/api/about", tag = "system", operation_id = "about",
    responses((status = 200, body = About)))]
pub(crate) async fn about(State(state): State<AppState>) -> Result<Json<About>, ApiError> {
    let modules = state.modules.report();
    let tools = state.tools.clone();
    let data_dir = state.data_dir.clone();
    let (tools, databases) = off_thread(move || {
        let databases = data_dir.as_deref().map(database_sizes).unwrap_or_default();
        (tools.probe(), databases)
    })
    .await?;
    Ok(Json(About {
        version: env!("CARGO_PKG_VERSION"),
        git_revision: option_env!("FMD2R_GIT_REVISION").filter(|r| !r.is_empty()),
        upstream_ref: modules.upstream_ref,
        upstream_sha: modules.upstream_sha,
        module_count: modules.module_count,
        load_failures: modules.load_failures,
        xpath_backend: modules.xpath_backend,
        data_dir: state.data_dir.as_deref().map(|d| d.display().to_string()),
        databases,
        uptime_secs: state.started.elapsed().as_secs(),
        tools,
    }))
}

fn database_sizes(dir: &Path) -> Vec<DatabaseSize> {
    let size = |name: &str| std::fs::metadata(dir.join(name)).ok().map(|m| m.len());
    DATABASES
        .into_iter()
        .map(|name| DatabaseSize {
            name,
            bytes: size(name).map(|db| db + size(&format!("{name}-wal")).unwrap_or(0)),
        })
        .collect()
}
