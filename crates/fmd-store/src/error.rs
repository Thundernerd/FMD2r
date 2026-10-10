use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{db} schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew {
        db: &'static str,
        found: u32,
        supported: u32,
    },
    #[error("{db} has schema version {found}; this build reads version {supported}")]
    SchemaMismatch {
        db: &'static str,
        found: u32,
        supported: u32,
    },
    #[error("invalid value {value:?} in column {column}")]
    InvalidColumn { column: &'static str, value: String },
    #[error("crypto: {0}")]
    Crypto(String),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;
