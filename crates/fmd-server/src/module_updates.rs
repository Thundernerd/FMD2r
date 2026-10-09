//! Starting the module updater with the server and running it on its schedule.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fmd_core::jobs::Job;
use fmd_core::module_updater::{
    LiveModules, ModuleUpdater, ModuleUpdaterJob, UpdaterConfig, has_no_modules,
};
use fmd_core::modules::StoreModuleSettings;
use fmd_core::settings::{ModuleUpdaterSettings, Settings, write_websitebypass_config};
use fmd_http::HttpClient;
use fmd_lua::{PoolConfig, WorkerPool};
use fmd_store::{AppDb, KeyFileCipher};
use tokio::time::Instant;

use crate::AppState;

/// Why the Lua modules could not be set up.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LuaRuntimeError {
    #[error("accounts key {path}: {source}")]
    KeyFile {
        path: PathBuf,
        source: fmd_store::StoreError,
    },
    #[error("HTTP client: {0}")]
    Http(#[from] fmd_http::HttpError),
    #[error("Lua workers: {0}")]
    Pool(#[from] std::io::Error),
}

/// The Lua modules the server runs: loaded from `<data dir>/lua`, run on one worker pool by the
/// download engine, and reloaded there by the module updater.
pub(crate) struct LuaRuntime {
    pub(crate) modules: Arc<LiveModules>,
    pub(crate) pool: Arc<WorkerPool>,
    pub(crate) http: HttpClient,
}

impl LuaRuntime {
    /// Loads the modules in `lua_dir` with their settings (options, cookies, accounts) read
    /// through `db`, credentials and cookies decrypted by the key in `key_file`. Blocks.
    pub(crate) fn load(
        db: AppDb,
        lua_dir: &Path,
        key_file: &Path,
    ) -> Result<LuaRuntime, LuaRuntimeError> {
        let cipher =
            KeyFileCipher::open_or_create(key_file).map_err(|source| LuaRuntimeError::KeyFile {
                path: key_file.to_owned(),
                source,
            })?;
        let modules = Arc::new(LiveModules::load(
            lua_dir,
            Arc::new(StoreModuleSettings::new(db, Arc::new(cipher))),
        ));
        let http = HttpClient::new()?;
        let mut config = PoolConfig::new(http.clone());
        config.lua_dir = lua_dir.to_owned();
        let pool = Arc::new(WorkerPool::new(config)?);
        Ok(LuaRuntime {
            modules,
            pool,
            http,
        })
    }
}

/// Registers the `modules` job over `runtime`'s modules, then runs it at startup and every
/// `module_updater.interval_minutes` while `module_updater.auto_update` is on. A tree with no
/// modules yet is synced at startup either way (the first-run bootstrap).
///
/// The repository, token and keep-last-good settings are read once: changes apply on the next
/// start. The FlareSolverr URL (`flaresolverr_override`, else the `connections.flaresolverr_url`
/// setting at that time) is written back into `websitebypass_config.json` whenever a sync
/// replaces it with upstream's.
pub(crate) fn start(
    state: AppState,
    runtime: &LuaRuntime,
    lua_dir: PathBuf,
    flaresolverr_override: Option<String>,
) {
    let settings = state.settings.get().module_updater.clone();
    let config = UpdaterConfig::from_settings(&settings, &lua_dir);
    let dir = lua_dir.clone();
    let settings_service = state.settings.clone();
    let updater = ModuleUpdater::new(
        config,
        state.db.clone(),
        runtime.http.clone(),
        runtime.modules.clone(),
    )
    .with_pool(runtime.pool.clone())
    .with_after_sync(move |report| {
        let flaresolverr_url =
            flaresolverr_url(flaresolverr_override.as_deref(), &settings_service.get());
        if report.downloaded.iter().any(|f| f == WEBSITEBYPASS_CONFIG)
            && let Err(e) = write_websitebypass_config(&dir, &flaresolverr_url)
        {
            tracing::warn!(target: "fmd_server", "writing {WEBSITEBYPASS_CONFIG}: {e}");
        }
    });
    let job = ModuleUpdaterJob::new(Arc::new(updater), state.jobs.clone());
    state.jobs.register(job.clone());
    state.jobs.changed(ModuleUpdaterJob::ID);
    tokio::spawn(schedule(job, state, lua_dir));
}

/// The FlareSolverr URL `websitebypass_config.json` points at: the flag or environment variable
/// (`flaresolverr_override`) for this run, else the stored `connections.flaresolverr_url`.
pub(crate) fn flaresolverr_url(flaresolverr_override: Option<&str>, settings: &Settings) -> String {
    flaresolverr_override
        .unwrap_or(&settings.connections.flaresolverr_url)
        .to_owned()
}

/// The config file `write_websitebypass_config` writes, relative to the Lua dir.
const WEBSITEBYPASS_CONFIG: &str = "websitebypass/websitebypass_config.json";

/// Runs `job` now when due, then on the interval, following setting changes.
async fn schedule(job: ModuleUpdaterJob, state: AppState, lua_dir: PathBuf) {
    let mut changes = state.settings.subscribe();
    let first = state.settings.get().module_updater.clone();
    if first.auto_update || has_no_modules(&lua_dir) {
        start_run(&job);
    }
    let mut last = Instant::now();
    let mut watching = true;
    loop {
        let settings = state.settings.get().module_updater.clone();
        let next = settings.auto_update.then(|| last + interval(&settings));
        job.set_next_run(next.map(unix_ms));
        let due = async {
            match next {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = due => {
                start_run(&job);
                last = Instant::now();
            }
            changed = changes.changed(), if watching => {
                // The settings service lives as long as the server; without it, keep the schedule.
                watching = changed.is_ok();
            }
        }
    }
}

fn start_run(job: &ModuleUpdaterJob) {
    // A run already going (started from the API) counts as this one.
    let _ = job.run();
}

fn interval(settings: &ModuleUpdaterSettings) -> Duration {
    Duration::from_secs(u64::from(settings.interval_minutes.max(1)) * 60)
}

/// `at` in Unix milliseconds.
fn unix_ms(at: Instant) -> i64 {
    let wall = SystemTime::now() + at.saturating_duration_since(Instant::now());
    wall.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
