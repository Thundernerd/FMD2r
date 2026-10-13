//! `GET /api/backup`: the "Download backup" button. A zip of what a user sets up: `app.db`, the
//! key its secrets are encrypted with, and the stylesheet. `lists.db` and `metadata.db` are left
//! out: they are large and rebuilt from the network.

use std::fs::File;
use std::io::{self, Seek, Write};
use std::path::Path;

use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use fmd_store::{ACCOUNTS_KEY_FILE, AppDb};
use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio_util::io::ReaderStream;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::state::off_thread;
use crate::{ApiError, AppState, Problem, custom_css};

/// The database's name in the data folder and in the zip.
const APP_DB: &str = "app.db";

/// What the backup is, for restoring it.
#[derive(Serialize)]
struct BackupInfo {
    fmd2r_version: &'static str,
    /// `app.db`'s `PRAGMA user_version`.
    schema_version: u32,
    /// RFC 3339, UTC.
    created_at: String,
}

/// Download a backup of the library, settings and saved logins, as
/// `fmd2r-backup-<YYYY-MM-DD>.zip`.
#[utoipa::path(get, path = "/api/backup", tag = "system", operation_id = "downloadBackup",
    responses(
        (status = 200, description = "app.db, accounts.key, backup.json and custom.css when present",
            content_type = "application/zip"),
        (status = 404, body = Problem, description = "The server runs without a data folder"),
    ))]
pub(crate) async fn download(State(state): State<AppState>) -> Result<Response, ApiError> {
    let dir = state
        .data_dir
        .clone()
        .ok_or_else(|| ApiError::Missing("the server has no data folder to back up".into()))?;
    let db = state.db.clone();
    let now = OffsetDateTime::now_utc();
    let file = off_thread(move || write_backup(&db, &dir, now)).await??;
    let len = file.metadata().map_err(io_error)?.len();
    let body = Body::from_stream(ReaderStream::new(tokio::fs::File::from_std(file)));
    let name = format!("fmd2r-backup-{}.zip", now.date());
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (header::CONTENT_LENGTH, len.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        body,
    )
        .into_response())
}

/// The zip, in an anonymous temp file.
fn write_backup(db: &AppDb, dir: &Path, now: OffsetDateTime) -> Result<File, ApiError> {
    let staging = tempfile::tempdir().map_err(io_error)?;
    let copy = staging.path().join(APP_DB);
    db.back_up_to(&copy)?;
    let info = BackupInfo {
        fmd2r_version: env!("CARGO_PKG_VERSION"),
        schema_version: db.schema_version()?,
        created_at: now
            .format(&Rfc3339)
            .map_err(|e| ApiError::Internal(e.to_string()))?,
    };
    let info = serde_json::to_vec_pretty(&info).map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut zip = ZipWriter::new(tempfile::tempfile().map_err(io_error)?);
    add_file(&mut zip, &copy, APP_DB)?;
    add_file(&mut zip, &dir.join(ACCOUNTS_KEY_FILE), ACCOUNTS_KEY_FILE)?;
    add_bytes(&mut zip, "backup.json", &info)?;
    let css = dir.join(custom_css::FILE);
    if css.is_file() {
        add_file(&mut zip, &css, custom_css::FILE)?;
    }
    let mut file = zip.finish().map_err(zip_error)?;
    file.rewind().map_err(io_error)?;
    Ok(file)
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true)
}

fn add_file(zip: &mut ZipWriter<File>, path: &Path, name: &str) -> Result<(), ApiError> {
    let mut source =
        File::open(path).map_err(|e| ApiError::Internal(format!("opening {name}: {e}")))?;
    zip.start_file(name, options()).map_err(zip_error)?;
    io::copy(&mut source, zip).map_err(|e| ApiError::Internal(format!("zipping {name}: {e}")))?;
    Ok(())
}

fn add_bytes(zip: &mut ZipWriter<File>, name: &str, bytes: &[u8]) -> Result<(), ApiError> {
    zip.start_file(name, options()).map_err(zip_error)?;
    zip.write_all(bytes).map_err(io_error)
}

fn io_error(err: io::Error) -> ApiError {
    ApiError::Internal(err.to_string())
}

fn zip_error(err: zip::result::ZipError) -> ApiError {
    ApiError::Internal(err.to_string())
}
