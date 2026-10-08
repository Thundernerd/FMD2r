//! Starting the module updater with the server and running it on its schedule.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fmd_core::jobs::Job;
use fmd_core::module_updater::{LiveModules, ModuleUpdater, ModuleUpdaterJob, UpdaterConfig};
use fmd_core::modules::StoreModuleSettings;
use fmd_core::settings::{ModuleUpdaterSettings, write_websitebypass_config};
use fmd_http::HttpClient;
use tokio::time::Instant;

use crate::AppState;

/// Loads the modules in `lua_dir`, registers the `modules` job, then runs it at startup and
/// every `module_updater.interval_minutes` while `module_updater.auto_update` is on. A tree with
/// no modules yet is synced at startup either way (the first-run bootstrap).
///
/// The repository, token and keep-last-good settings are read once: changes apply on the next
/// start. `flaresolverr_url` is written back into `websitebypass_config.json` whenever a sync
/// replaces it with upstream's.
pub(crate) async fn start(state: AppState, lua_dir: PathBuf, flaresolverr_url: String) {
    let settings = state.settings.get().module_updater.clone();
    let db = state.db.clone();
    let jobs = state.jobs.clone();
    let dir = lua_dir.clone();
    let job = tokio::task::spawn_blocking(move || -> Result<ModuleUpdaterJob, String> {
        let modules = Arc::new(LiveModules::load(
            &dir,
            Arc::new(StoreModuleSettings::new(db.clone())),
        ));
        let http = HttpClient::new().map_err(|e| e.to_string())?;
        let config = UpdaterConfig::from_settings(&settings, &dir);
        let updater = ModuleUpdater::new(config, db, http, modules).with_after_sync({
            let dir = dir.clone();
            move |report| {
                if report.downloaded.iter().any(|f| f == WEBSITEBYPASS_CONFIG)
                    && let Err(e) = write_websitebypass_config(&dir, &flaresolverr_url)
                {
                    tracing::warn!(target: "fmd_server", "writing {WEBSITEBYPASS_CONFIG}: {e}");
                }
            }
        });
        Ok(ModuleUpdaterJob::new(Arc::new(updater), jobs))
    })
    .await;
    let job = match job {
        Ok(Ok(job)) => job,
        Ok(Err(e)) => {
            tracing::error!(target: "fmd_server", "module updater: {e}");
            return;
        }
        Err(e) => {
            tracing::error!(target: "fmd_server", "module updater: {e}");
            return;
        }
    };
    state.jobs.register(job.clone());
    state.jobs.changed(ModuleUpdaterJob::ID);
    schedule(job, state, lua_dir).await;
}

/// The config file `write_websitebypass_config` writes, relative to the Lua dir.
const WEBSITEBYPASS_CONFIG: &str = "websitebypass/websitebypass_config.json";

/// Runs `job` now when due, then on the interval, following setting changes.
async fn schedule(job: ModuleUpdaterJob, state: AppState, lua_dir: PathBuf) {
    let mut changes = state.settings.subscribe();
    let first = state.settings.get().module_updater.clone();
    if first.auto_update || no_modules(&lua_dir) {
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

/// Whether `<lua_dir>/modules` holds nothing yet.
fn no_modules(lua_dir: &std::path::Path) -> bool {
    std::fs::read_dir(lua_dir.join("modules")).map_or(true, |mut e| e.next().is_none())
}

/// `at` in Unix milliseconds.
fn unix_ms(at: Instant) -> i64 {
    let wall = SystemTime::now() + at.saturating_duration_since(Instant::now());
    wall.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
