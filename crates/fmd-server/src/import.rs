//! `POST /api/import`: imports a zipped FMD2 `userdata` folder, as `fmd2r import` does with a
//! folder on disk.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use axum::Json;
use axum::body::Body;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, header};
use fmd_core::jobs::{Job, JobError, JobPhase, JobRegistry, JobStatus};
use fmd_import::{ImportError, ImportOptions, ImportReport, TimeZone};
use fmd_store::{ACCOUNTS_KEY_FILE, KeyFileCipher};
use futures_util::StreamExt;
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{KnownFormat, ObjectBuilder, Schema, SchemaFormat, Type};
use utoipa::{IntoParams, PartialSchema, ToSchema};

use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

/// How big a `POST /api/import` upload may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportLimits {
    /// The zip itself.
    pub upload_bytes: u64,
    /// Its entries, unpacked, together.
    pub extracted_bytes: u64,
}

impl Default for ImportLimits {
    /// 512 MiB zipped, 2 GiB unpacked: FMD2's databases compress well, and a large userdata
    /// folder is a few hundred MiB.
    fn default() -> Self {
        Self {
            upload_bytes: 512 << 20,
            extracted_bytes: 2 << 30,
        }
    }
}

/// The options of an import, as `fmd2r import` takes them.
#[derive(Debug, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ImportQuery {
    /// Report what would be imported without writing anything.
    dry_run: Option<bool>,
    /// Queue tasks FMD2 was running as waiting, so they resume, instead of stopped.
    resume: Option<bool>,
    /// `FROM=TO`: rewrite save-to paths under FROM to TO, e.g. `C:\Manga=/data/manga`.
    /// Repeatable; the longest matching FROM wins.
    #[param(value_type = Option<Vec<String>>)]
    map_path: Vec<String>,
    /// The IANA time zone FMD2 ran in, e.g. `Europe/Amsterdam`: FMD2 stores local times without
    /// a zone. The server's zone by default.
    timezone: Option<String>,
}

impl ImportQuery {
    /// Parsed by hand because `map_path` may repeat.
    fn parse(query: Option<&str>) -> Result<Self, ApiError> {
        let mut out = Self::default();
        for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
            match &*key {
                "dry_run" => out.dry_run = Some(flag(&key, &value)?),
                "resume" => out.resume = Some(flag(&key, &value)?),
                "map_path" => out.map_path.push(value.into_owned()),
                "timezone" => out.timezone = Some(value.into_owned()),
                _ => {}
            }
        }
        Ok(out)
    }

    fn options(self) -> Result<ImportOptions, ApiError> {
        let path_maps = self
            .map_path
            .iter()
            .map(|m| {
                m.parse().map_err(|e| ApiError::Invalid {
                    field: Some("map_path".into()),
                    detail: format!("{e}"),
                })
            })
            .collect::<Result<_, _>>()?;
        let timezone = match self.timezone.as_deref() {
            None | Some("") => TimeZone::system(),
            Some(name) => TimeZone::named(name).map_err(|e| ApiError::Invalid {
                field: Some("timezone".into()),
                detail: e.to_string(),
            })?,
        };
        Ok(ImportOptions {
            dry_run: self.dry_run.unwrap_or(false),
            resume_in_progress: self.resume.unwrap_or(false),
            path_maps,
            timezone,
        })
    }
}

fn flag(key: &str, value: &str) -> Result<bool, ApiError> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(ApiError::Invalid {
            field: Some(key.into()),
            detail: format!("expected true or false, got {value:?}"),
        }),
    }
}

/// A zip file as the request body: `string` with format `binary`, which the derive does not
/// produce for bytes.
struct ZipFile;

impl PartialSchema for ZipFile {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
            .into()
    }
}

impl ToSchema for ZipFile {}

/// Import an FMD2 `userdata` folder, zipped, into this server.
#[utoipa::path(post, path = "/api/import", tag = "system", operation_id = "importFmd2",
    params(ImportQuery),
    request_body(content = inline(ZipFile), content_type = "application/zip",
        description = "The FMD2 `userdata` folder (or its contents) as a zip"),
    responses(
        (status = 200, body = fmd_import::ImportReport, description = "What was imported, or with `dry_run` what would be"),
        (status = 400, description = "Not a zip, an entry outside the folder, or an unreadable FMD2 file", body = Problem),
        (status = 409, description = "An import is already running", body = Problem),
        (status = 413, description = "The zip, or what it unpacks to, is over the size limit", body = Problem),
        (status = 422, description = "A bad `map_path` or `timezone`", body = Problem),
        (status = 503, description = "The server has no data directory", body = Problem),
    ))]
pub(crate) async fn import(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<ImportReport>, ApiError> {
    let opts = ImportQuery::parse(query.as_deref())?.options()?;
    let data_dir = state
        .data_dir
        .clone()
        .ok_or_else(|| ApiError::Unavailable("the server has no data directory".into()))?;
    let limit = state.import_limits.upload_bytes;
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|len| len > limit) {
        return Err(too_large("the upload", limit));
    }
    let run = ImportRun::start(&state)?;
    let scratch = match receive(body, &data_dir, limit).await {
        Ok(scratch) => scratch,
        Err(e) => {
            run.finish(Some(&e));
            return Err(e);
        }
    };
    // From here on the import finishes, and its job ends, whether or not the client still waits.
    tokio::spawn(async move {
        let outcome = import_upload(&state, scratch, data_dir, opts, &run).await;
        run.finish(outcome.as_ref().err());
        outcome
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?
    .map(Json)
}

async fn import_upload(
    state: &AppState,
    scratch: TempDir,
    data_dir: PathBuf,
    opts: ImportOptions,
    run: &ImportRun,
) -> Result<ImportReport, ApiError> {
    let limits = state.import_limits;
    let db = state.db.clone();
    let settings = state.settings.clone();
    let run = run.clone();
    let report = off_thread(move || -> Result<ImportReport, ApiError> {
        let userdata = extract(&scratch, limits.extracted_bytes)?;
        let cipher = KeyFileCipher::open_or_create(data_dir.join(ACCOUNTS_KEY_FILE))?;
        fmd_import::import_into(&userdata, &db, &cipher, &settings, &opts, |p| {
            run.progress(p.done, p.total)
        })
        .map_err(import_error)
    })
    .await??;
    if !report.dry_run {
        // The tasks bypassed the engine; start the waiting ones now, not at the next start.
        if let Err(e) = state.engine.activate_waiting().await {
            tracing::warn!(target: "fmd_server", "starting imported tasks: {e}");
        }
    }
    Ok(report)
}

/// An unreadable FMD2 file is the upload's fault: name it (not its temp path) and say why.
fn import_error(e: ImportError) -> ApiError {
    let name = |path: &Path| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match e {
        ImportError::Sqlite { path, source } => {
            ApiError::BadRequest(format!("{}: {source}", name(&path)))
        }
        ImportError::Json { path, reason } => {
            ApiError::BadRequest(format!("{}: {reason}", name(&path)))
        }
        ImportError::Store(e) => ApiError::Store(e),
        e @ ImportError::Settings(_) => ApiError::Internal(e.to_string()),
    }
}

/// Saves the body as [`UPLOAD`] in a new folder in `data_dir`, refusing more than `limit` bytes.
async fn receive(body: Body, data_dir: &Path, limit: u64) -> Result<TempDir, ApiError> {
    let saving = |e: io::Error| ApiError::Internal(format!("saving the upload: {e}"));
    let scratch = tempfile::Builder::new()
        .prefix(".import-")
        .tempdir_in(data_dir)
        .map_err(saving)?;
    let mut file = tokio::fs::File::create(scratch.path().join(UPLOAD))
        .await
        .map_err(saving)?;
    let mut stream = body.into_data_stream();
    let mut size = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::BadRequest(format!("reading the upload: {e}")))?;
        size += chunk.len() as u64;
        if size > limit {
            return Err(too_large("the upload", limit));
        }
        file.write_all(&chunk).await.map_err(saving)?;
    }
    file.flush().await.map_err(saving)?;
    Ok(scratch)
}

/// The uploaded zip's name in the upload folder.
const UPLOAD: &str = "upload.zip";

/// Zip entries an upload may have: a userdata folder has a handful of files.
const MAX_ENTRIES: usize = 10_000;

fn too_large(what: &str, limit: u64) -> ApiError {
    let limit = if limit >= 1 << 20 {
        format!("{} MiB", limit >> 20)
    } else {
        format!("{limit} bytes")
    };
    ApiError::PayloadTooLarge(format!("{what} is larger than {limit}"))
}

/// Returns the extracted `userdata` folder, refusing more than `limit` bytes or [`MAX_ENTRIES`]
/// entries.
fn extract(scratch: &TempDir, limit: u64) -> Result<PathBuf, ApiError> {
    let bad = |e: zip::result::ZipError| ApiError::BadRequest(format!("not a zip file: {e}"));
    let upload = File::open(scratch.path().join(UPLOAD)).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(upload).map_err(bad)?;
    if archive.len() > MAX_ENTRIES {
        return Err(ApiError::PayloadTooLarge(format!(
            "the upload has more than {MAX_ENTRIES} entries"
        )));
    }
    let root = scratch.path().join("userdata");
    let mut left = limit;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(bad)?;
        let Some(name) = entry.enclosed_name().filter(|_| !is_absolute(entry.name())) else {
            return Err(ApiError::BadRequest(format!(
                "zip entry {:?} lies outside the folder",
                entry.name()
            )));
        };
        let target = root.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(io_error)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut out = File::create(&target).map_err(io_error)?;
        // The declared size may lie, so count what actually comes out.
        let written = io::copy(&mut (&mut entry).take(left + 1), &mut out).map_err(io_error)?;
        if written > left {
            return Err(too_large("the unpacked upload", limit));
        }
        left -= written;
    }
    Ok(userdata_folder(root))
}

/// `/…`, `\…` or `X:…`: `enclosed_name` would quietly make these relative.
fn is_absolute(name: &str) -> bool {
    let b = name.as_bytes();
    matches!(b.first(), Some(b'/' | b'\\'))
        || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

fn io_error(e: io::Error) -> ApiError {
    ApiError::Internal(format!("extracting the upload: {e}"))
}

/// The files `fmd_import::import` reads (baseunits/FMDOptions.pas:288-295).
const USERDATA_FILES: &[&str] = &[
    "downloads.db",
    "favorites.db",
    "downloadedchapters.db",
    "modules.json",
    "settings.json",
];

/// `dir`, or the folder inside it when the zip holds the `userdata` folder itself rather than its
/// contents.
fn userdata_folder(mut dir: PathBuf) -> PathBuf {
    loop {
        if USERDATA_FILES.iter().any(|f| dir.join(f).is_file()) {
            return dir;
        }
        let mut subdirs = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()));
        match (subdirs.next(), subdirs.next()) {
            (Some(only), None) => dir = only.path(),
            _ => return dir,
        }
    }
}

/// The `import` job, for the System page; it only runs through `POST /api/import`.
#[derive(Clone)]
pub(crate) struct ImportJob(Arc<Mutex<JobStatus>>);

impl Default for ImportJob {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(fresh_status(JobPhase::Idle, None))))
    }
}

impl ImportJob {
    pub(crate) const ID: &str = "import";

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Starts a run, unless one is running.
    fn try_begin(&self) -> Result<(), JobError> {
        let mut current = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if current.phase == JobPhase::Running {
            return Err(JobError::AlreadyRunning);
        }
        *current = fresh_status(JobPhase::Running, Some(now_ms()));
        Ok(())
    }
}

/// A run's status before any progress.
fn fresh_status(phase: JobPhase, last_run: Option<i64>) -> JobStatus {
    JobStatus {
        phase,
        done: 0,
        total: 0,
        last_run,
        next_run: None,
        last_error: None,
    }
}

impl Job for ImportJob {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "Import from FMD2"
    }

    fn status(&self) -> JobStatus {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn run(&self) -> Result<(), JobError> {
        Err(JobError::Unsupported(
            "an import needs an upload: POST it to /api/import".into(),
        ))
    }

    fn cancel(&self) -> Result<(), JobError> {
        match self.status().phase {
            JobPhase::Running => Err(JobError::Unsupported(
                "an import cannot be cancelled".into(),
            )),
            _ => Err(JobError::NotRunning),
        }
    }
}

/// The running import. Dropping it before [`ImportRun::finish`] (the client went away) ends the
/// run as failed.
#[derive(Clone)]
struct ImportRun(Arc<RunInner>);

struct RunInner {
    job: ImportJob,
    registry: JobRegistry,
}

impl ImportRun {
    /// Registers the job on its first run; a conflict if an import is running.
    fn start(state: &AppState) -> Result<Self, ApiError> {
        let job = state.import_job.clone();
        job.try_begin()?;
        if state.jobs.get(ImportJob::ID).is_none() {
            state.jobs.register(job.clone());
        }
        state.jobs.changed(ImportJob::ID);
        Ok(Self(Arc::new(RunInner {
            job,
            registry: state.jobs.clone(),
        })))
    }

    fn progress(&self, done: u64, total: u64) {
        self.0.job.update(|s| {
            s.done = done;
            s.total = total;
        });
        self.0.registry.changed(ImportJob::ID);
    }

    fn finish(&self, error: Option<&ApiError>) {
        self.0.end(error.map(ToString::to_string));
    }
}

impl RunInner {
    /// Ends the run unless it has ended already.
    fn end(&self, error: Option<String>) {
        let mut ended = false;
        self.job.update(|s| {
            if s.phase == JobPhase::Running {
                s.phase = if error.is_some() {
                    JobPhase::Failed
                } else {
                    JobPhase::Done
                };
                s.last_error = error;
                ended = true;
            }
        });
        if ended {
            self.registry.changed(ImportJob::ID);
        }
    }
}

impl Drop for RunInner {
    fn drop(&mut self) {
        self.end(Some("the upload was interrupted".into()));
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
