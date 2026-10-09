//! List metadata from MangaBaka's database (T73): an opt-in local copy of its nightly dump
//! ([`MangaBakaDb`]), and list titles matched against it offline.

mod build;
mod db;
mod jobs;
mod matcher;
mod normalize;
mod source;

use thiserror::Error;

pub use build::{BuildProgress, BuildSummary};
pub use db::{DUMP_URL, DbInfo, METADATA_DB, MangaBakaDb, Metadata};
pub use jobs::{
    MetadataEvent, MetadataEventKind, MetadataJobError, MetadataJobs, MetadataPhase, ModuleRoots,
};
pub use matcher::{ListModule, MangaDexLinks, MatchReport, MatchScope, Matcher};
pub use source::{Download, DumpSource, HttpDumpSource, USER_AGENT};

/// Why building `metadata.db` (or matching against it) failed. A failed build leaves the
/// database it would have replaced as it was.
#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("downloading {url} failed with HTTP status {status}")]
    Download { url: String, status: u16 },
    #[error("HTTP: {0}")]
    Http(String),
    #[error("the download was cancelled")]
    Cancelled,
    #[error("reading the dump: {0}")]
    Read(std::io::Error),
    #[error("line {line} of the dump is not valid JSON: {source}")]
    Json {
        line: u64,
        source: serde_json::Error,
    },
    #[error("line {line} of the dump is not a JSON object")]
    NotAnObject { line: u64 },
    /// MangaBaka changed its dump in a way matching cannot follow.
    #[error(
        "MangaBaka's database format changed: series {} (line {line}) has no `{field}` field, \
         which matching needs. FMD2r needs an update to read it.",
        id.map_or_else(|| "?".to_owned(), |id| id.to_string())
    )]
    MissingField {
        field: String,
        id: Option<i64>,
        line: u64,
    },
    #[error("line {line} of the dump has an unusable `{field}` field")]
    BadField { field: String, line: u64 },
    #[error("the dump holds no active series")]
    Empty,
    #[error("metadata.db: {0}")]
    Store(#[from] fmd_store::StoreError),
    #[error("lists.db: {0}")]
    Lists(fmd_store::StoreError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}
