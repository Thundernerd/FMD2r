//! Restoring a backup from `GET /api/backup`: `fmd2r restore` while the server is stopped, and
//! the setup wizard on a fresh install, whose upload is staged in the data folder and applied at
//! the next start, before the stores open.

use std::fs::File;
use std::io::{self, Read, Seek};
use std::path::Path;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use fmd_core::settings::check_settings_key;
use fmd_store::{ACCOUNTS_KEY_FILE, AppDb, StoreError};
use thiserror::Error;

use crate::backup::{APP_DB, BACKUP_JSON};
use crate::import::{UPLOAD, ZipFile, receive, refuse_declared_over, too_large};
use crate::state::off_thread;
use crate::{ApiError, AppState, ImportLimits, Problem, custom_css};

/// The folder in the data folder holding a validated backup until the next start.
const STAGED: &str = "restore-staged";

/// What the old files are kept as: `app.db` becomes `app.db.before-restore`.
const BEFORE_RESTORE: &str = ".before-restore";

/// The files a backup must hold.
const REQUIRED: [&str; 3] = [APP_DB, ACCOUNTS_KEY_FILE, BACKUP_JSON];

#[derive(Debug, Error)]
pub enum RestoreError {
    #[error("not a zip file: {0}")]
    NotZip(String),
    #[error("not a backup from FMD2r: it has no {0}")]
    Missing(&'static str),
    #[error("This backup is from a newer FMD2r; update first.")]
    TooNew,
    #[error("the backup's app.db does not open: {0}")]
    Unreadable(StoreError),
    #[error("the backup's accounts.key does not decrypt its saved secrets: {0}")]
    WrongKey(StoreError),
    #[error("the backup unpacks to more than {} MiB", .0 >> 20)]
    TooLarge(u64),
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl From<RestoreError> for ApiError {
    fn from(e: RestoreError) -> Self {
        match e {
            RestoreError::TooLarge(limit) => too_large("the unpacked backup", limit),
            RestoreError::Io(e) => ApiError::Internal(format!("restoring the backup: {e}")),
            refused => ApiError::BadRequest(refused.to_string()),
        }
    }
}

/// Restore a backup from `GET /api/backup` during setup: the backup is validated and staged,
/// and replaces this server's data at its next start. Only a server that is not set up yet takes
/// one; restore over a set-up install with `fmd2r restore` while the server is stopped.
#[utoipa::path(post, path = "/api/setup/restore", tag = "system", operation_id = "restoreBackup",
    request_body(content = inline(ZipFile), content_type = "application/zip",
        description = "A backup from `GET /api/backup`"),
    responses(
        (status = 204, description = "The backup is valid and staged: restart FMD2r to finish restoring"),
        (status = 400, body = Problem, description = "Not a backup, one from a newer FMD2r, or one whose key does not decrypt its secrets"),
        (status = 409, body = Problem, description = "The server is set up already"),
        (status = 413, body = Problem, description = "The zip, or what it unpacks to, is over the size limit"),
        (status = 503, body = Problem, description = "The server has no data directory"),
    ))]
pub(crate) async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, ApiError> {
    if state.settings.get().general.setup_completed {
        return Err(ApiError::Conflict(
            "FMD2r is set up already: stop it and restore with `fmd2r restore`".into(),
        ));
    }
    let data_dir = state
        .data_dir
        .clone()
        .ok_or_else(|| ApiError::Unavailable("the server has no data directory".into()))?;
    let limits = state.import_limits;
    refuse_declared_over(&headers, limits.upload_bytes)?;
    let scratch = receive(body, &data_dir, limits.upload_bytes).await?;
    off_thread(move || -> Result<(), ApiError> {
        let zip = File::open(scratch.path().join(UPLOAD))
            .map_err(|e| ApiError::Internal(format!("reading the upload: {e}")))?;
        stage(zip, &data_dir, limits.extracted_bytes)?;
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

/// Validates the backup `zip` and replaces `app.db`, `accounts.key` and `custom.css` in
/// `data_dir` with its files, keeping the old ones as `*.before-restore`. Nothing changes when
/// the backup is refused. Run it while the server is stopped. Blocking.
pub fn restore_backup(zip: &Path, data_dir: &Path) -> Result<(), RestoreError> {
    std::fs::create_dir_all(data_dir)?;
    let limit = ImportLimits::default().extracted_bytes;
    stage(File::open(zip)?, data_dir, limit)?;
    apply_staged_restore(data_dir)?;
    Ok(())
}

/// Validates the backup `zip` and stages its files in `data_dir` for
/// [`apply_staged_restore`], replacing a backup staged before. Refuses more than `limit` bytes
/// unpacked. Blocking.
fn stage(zip: impl Read + Seek, data_dir: &Path, limit: u64) -> Result<(), RestoreError> {
    let mut archive = zip::ZipArchive::new(zip).map_err(|e| RestoreError::NotZip(e.to_string()))?;
    let scratch = tempfile::Builder::new()
        .prefix(".restore-")
        .tempdir_in(data_dir)?;
    let mut left = limit;
    for name in REQUIRED {
        extract(&mut archive, name, scratch.path(), &mut left, limit)?
            .then_some(())
            .ok_or(RestoreError::Missing(name))?;
    }
    extract(
        &mut archive,
        custom_css::FILE,
        scratch.path(),
        &mut left,
        limit,
    )?;
    validate(scratch.path())?;
    let staged = data_dir.join(STAGED);
    if staged.exists() {
        std::fs::remove_dir_all(&staged)?;
    }
    // The emptied temp dir's cleanup then finds nothing to remove.
    std::fs::rename(scratch.path(), &staged)?;
    Ok(())
}

/// Opens the extracted `app.db`, which migrates an older one, and decrypts its secrets with the
/// extracted key.
fn validate(dir: &Path) -> Result<(), RestoreError> {
    let db = match AppDb::open(dir.join(APP_DB)) {
        Ok(db) => db,
        Err(StoreError::SchemaTooNew { .. }) => return Err(RestoreError::TooNew),
        Err(e) => return Err(RestoreError::Unreadable(e)),
    };
    let decrypts = |checked: Result<(), StoreError>| match checked {
        Err(e @ StoreError::Crypto(_)) => Err(RestoreError::WrongKey(e)),
        other => other.map_err(RestoreError::Unreadable),
    };
    decrypts(db.accounts(db.cipher()).check_key())?;
    decrypts(check_settings_key(&db))
}

/// Writes the entry `name` to `dir`, counting it against the `left` of `limit` bytes; `false`
/// when the zip has none.
fn extract(
    archive: &mut zip::ZipArchive<impl Read + Seek>,
    name: &str,
    dir: &Path,
    left: &mut u64,
    limit: u64,
) -> Result<bool, RestoreError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(false),
        Err(e) => return Err(RestoreError::NotZip(e.to_string())),
    };
    // The declared size may lie, so count what actually comes out.
    let mut out = File::create(dir.join(name))?;
    let written = io::copy(&mut (&mut entry).take(*left + 1), &mut out)?;
    if written > *left {
        return Err(RestoreError::TooLarge(limit));
    }
    *left -= written;
    Ok(true)
}

/// Moves a staged backup's files into `data_dir`, keeping the old ones as `*.before-restore`;
/// `false` when none is staged. Run it before the stores open. Blocking.
pub fn apply_staged_restore(data_dir: &Path) -> Result<bool, RestoreError> {
    let staged = data_dir.join(STAGED);
    if !staged.is_dir() {
        return Ok(false);
    }
    // Each file is moved aside and replaced in turn, so a start interrupted halfway finishes the
    // remaining files at the next one.
    for name in [APP_DB, ACCOUNTS_KEY_FILE] {
        let new = staged.join(name);
        if new.exists() {
            set_aside(data_dir, name)?;
            std::fs::rename(new, data_dir.join(name))?;
        }
    }
    // A backup without a stylesheet restores the default one.
    set_aside(data_dir, custom_css::FILE)?;
    let css = staged.join(custom_css::FILE);
    if css.exists() {
        std::fs::rename(css, data_dir.join(custom_css::FILE))?;
    }
    std::fs::remove_dir_all(staged)?;
    Ok(true)
}

/// Renames `name` to `name.before-restore`, `app.db`'s WAL and shared-memory files along with
/// it (as `app.db.before-restore-wal`, so the old database still opens).
fn set_aside(dir: &Path, name: &str) -> io::Result<()> {
    let companions: &[&str] = if name == APP_DB {
        &["", "-wal", "-shm"]
    } else {
        &[""]
    };
    for suffix in companions {
        let from = dir.join(format!("{name}{suffix}"));
        if from.exists() {
            replace(&from, &dir.join(format!("{name}{BEFORE_RESTORE}{suffix}")))?;
        }
    }
    Ok(())
}

/// Renames `from` to `to`, replacing `to` (which `rename` won't on Windows).
fn replace(from: &Path, to: &Path) -> io::Result<()> {
    if to.exists() {
        std::fs::remove_file(to)?;
    }
    std::fs::rename(from, to)
}
