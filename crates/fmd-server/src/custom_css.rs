//! `GET /custom.css`: the user's stylesheet from the data folder, loaded after the app's own.

use std::io::ErrorKind;
use std::path::Path;

use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use tokio::io::AsyncReadExt;

use crate::{ApiError, AppState};

/// The file in the data folder.
const FILE: &str = "custom.css";

/// The largest stylesheet served; anything bigger is a 413.
const MAX_BYTES: u64 = 1024 * 1024;

/// Read on every request, so edits show on the next reload. No file, or no data folder, is an
/// empty stylesheet rather than a 404, so the browser console stays clean.
pub(crate) async fn get(State(state): State<AppState>) -> Result<Response, ApiError> {
    let body = match state.data_dir.as_deref() {
        Some(dir) => read(&dir.join(FILE)).await?,
        None => Vec::new(),
    };
    Ok((
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response())
}

/// The file's bytes, empty when it doesn't exist.
async fn read(path: &Path) -> Result<Vec<u8>, ApiError> {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(ApiError::Internal(format!("opening {FILE}: {e}"))),
    };
    let mut body = Vec::new();
    // One byte past the limit tells an oversized file apart without reading all of it.
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut body)
        .await
        .map_err(|e| ApiError::Internal(format!("reading {FILE}: {e}")))?;
    if body.len() as u64 > MAX_BYTES {
        return Err(ApiError::PayloadTooLarge(format!(
            "{FILE} is larger than {MAX_BYTES} bytes"
        )));
    }
    Ok(body)
}
