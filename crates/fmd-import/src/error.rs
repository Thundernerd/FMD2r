use std::path::PathBuf;

use thiserror::Error;

/// Errors that stop an import.
#[derive(Debug, Error)]
pub enum ImportError {
    /// An FMD2 database could not be read.
    #[error("{path}: {source}")]
    Sqlite {
        path: PathBuf,
        source: rusqlite::Error,
    },
    /// An FMD2 JSON file could not be read or parsed.
    #[error("{path}: {reason}")]
    Json { path: PathBuf, reason: String },
    #[error("store: {0}")]
    Store(#[from] fmd_store::StoreError),
    #[error("settings: {0}")]
    Settings(#[from] fmd_core::settings::SettingsError),
}
