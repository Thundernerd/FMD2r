//! Services later tickets plug into [`crate::AppState`], kept behind traits so the server does
//! not depend on the engine crates.

/// The download engine (T20). Queue endpoints (T23) add the methods they need.
pub trait DownloadEngine: Send + Sync + 'static {}

/// Background jobs: favorites check, list update, module update (T25, T26, T29, T36).
pub trait Jobs: Send + Sync + 'static {}

/// Stand-in for both until the real services exist: no tasks, no jobs.
pub struct Idle;

impl DownloadEngine for Idle {}
impl Jobs for Idle {}
