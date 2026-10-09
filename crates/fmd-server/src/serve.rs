//! Running the server on a socket until SIGINT/SIGTERM.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use fmd_core::accounts::AccountService;
use fmd_core::download::{DownloadManager, EngineConfig, ModuleLookup};
use fmd_core::favorites::{CheckerConfig, FavoritesChecker, TaskQueue};
use fmd_core::lists::{DbImporter, ListJobs, ListUpdater};
use fmd_core::settings::{SettingsService, write_websitebypass_config};
use fmd_store::{ACCOUNTS_KEY_FILE, AppDb, ListsDb};
use thiserror::Error;
use tokio::net::TcpListener;

use crate::events::ServerEvent;
use crate::lua_catalog::LuaCatalog;
use crate::module_updates::{self, LuaRuntime};
use crate::{AppState, CoverConfig, Idle, LogBuffer, LogRotation, SystemTools, build_router};

/// What [`serve`] needs.
pub struct ServeConfig {
    pub bind: SocketAddr,
    /// Holds `app.db`, `lists.db`, the Lua tree (`lua/`), the cover cache (`covers/`) and the log
    /// files (`logs/`); created when missing.
    pub data_dir: PathBuf,
    /// Password/token required for the API; `None` leaves it open.
    pub auth: Option<String>,
    /// Used instead of the stored `connections.flaresolverr_url` setting when startup writes
    /// `lua/websitebypass/websitebypass_config.json`; the setting itself is left as it is.
    pub flaresolverr_url: Option<String>,
    /// The buffer the `tracing` subscriber feeds; `GET /api/logs` reads it and `GET /api/events`
    /// streams its bus. It is persisted to `<data dir>/logs/` as the `logs` settings say.
    pub logs: LogBuffer,
    /// Keep the Lua modules in sync with upstream (the `modules` job and its schedule). Off, no
    /// module updater runs and nothing is fetched from GitHub; the modules already in
    /// `<data dir>/lua` are still loaded for the download engine.
    pub module_updates: bool,
}

/// Errors that stop [`serve`].
#[derive(Debug, Error)]
pub enum ServeError {
    #[error("data dir {path}: {source}")]
    DataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("app.db or lists.db: {0}")]
    Store(#[from] fmd_store::StoreError),
    #[error("settings: {0}")]
    Settings(#[from] fmd_core::settings::SettingsError),
    #[error("bind {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("server: {0}")]
    Io(#[from] std::io::Error),
}

/// Opens the store in `config.data_dir`, then serves the app on `config.bind` until SIGINT or
/// SIGTERM, letting in-flight requests finish and closing event streams.
pub async fn serve(config: ServeConfig) -> Result<(), ServeError> {
    std::fs::create_dir_all(&config.data_dir).map_err(|source| ServeError::DataDir {
        path: config.data_dir.clone(),
        source,
    })?;
    let db_path = config.data_dir.join("app.db");
    let lists_path = config.data_dir.join("lists.db");
    // Opening the stores and loading the settings block.
    let state = tokio::task::spawn_blocking(move || -> Result<AppState, ServeError> {
        let lists = ListsDb::open(lists_path)?;
        Ok(AppState::new(AppDb::open(db_path)?)?.with_lists(lists))
    })
    .await
    .map_err(std::io::Error::other)??;
    // Read once: cover cache and log rotation changes apply on the next start.
    let settings = state.settings.get();
    // Absolute, so `GET /api/about` shows where the data really is.
    let data_dir = std::fs::canonicalize(&config.data_dir).unwrap_or(config.data_dir);
    let lua_dir = data_dir.join("lua");
    // The flag or environment variable wins for this run without replacing the stored setting.
    let flaresolverr_override = config.flaresolverr_url;
    let flaresolverr_url =
        module_updates::flaresolverr_url(flaresolverr_override.as_deref(), &settings);
    // Where upstream's cloudflare.lua looks for FlareSolverr (lua/websitebypass/cloudflare.lua:271-325).
    if let Err(e) = write_websitebypass_config(&lua_dir, &flaresolverr_url) {
        tracing::warn!(target: "fmd_server", "writing websitebypass_config.json: {e}");
    }
    let bypass_config = lua_dir.join("websitebypass/websitebypass_config.json");
    // Loading the persisted log tail blocks.
    let logs = config.logs.clone();
    let logs_dir = data_dir.join("logs");
    let rotation = LogRotation::from_settings(&settings.logs);
    let persisted = tokio::task::spawn_blocking(move || logs.persist(&logs_dir, rotation))
        .await
        .map_err(std::io::Error::other)
        .and_then(|r| r);
    if let Err(e) = persisted {
        tracing::warn!(target: "fmd_server", "persisting logs: {e}");
    }
    let covers = CoverConfig::from_settings(data_dir.join("covers"), &settings.covers);
    let mut state = state
        .with_logs(config.logs)
        .with_data_dir(&data_dir)
        .with_tools(SystemTools::new(bypass_config));
    if let Some(secret) = config.auth {
        state = state.with_auth(secret);
    }
    // Loading the modules and resuming the downloads that were running block.
    let db = state.db.clone();
    let key_file = data_dir.join(ACCOUNTS_KEY_FILE);
    let dir = lua_dir.clone();
    let xpath_backend = settings.xpath.backend;
    let runtime =
        tokio::task::spawn_blocking(move || LuaRuntime::load(db, &dir, &key_file, xpath_backend))
            .await
            .map_err(std::io::Error::other)?;
    match runtime {
        Ok(runtime) => {
            module_updates::follow_xpath_backend(state.settings.clone(), &runtime);
            let catalog = LuaCatalog::new(&runtime, state.db.clone());
            // The accounts are the modules' `MODULE.Account`, stored encrypted under the key
            // file the runtime loaded them with.
            let live = runtime.modules.clone();
            let accounts = AccountService::following(move || live.current(), runtime.pool.clone());
            state = state
                .with_modules(catalog.clone())
                .with_covers(covers, catalog)
                .with_accounts(Arc::new(accounts));
            let live = runtime.modules.clone();
            let modules: Arc<ModuleLookup> =
                Arc::new(move |id: &str| live.current().get(id).cloned());
            if let Some(jobs) = list_jobs(&state, &runtime, modules.clone()) {
                jobs.register(&state.jobs);
                state = state.with_list_jobs(jobs);
            }
            let engine = DownloadManager::open(EngineConfig {
                db: state.db.clone(),
                pool: runtime.pool.clone(),
                modules: modules.clone(),
                settings: state.settings.clone(),
                http: runtime.http.clone(),
            })
            .await;
            let queue = match engine {
                Ok(engine) => {
                    let engine = Arc::new(engine);
                    state = state.with_shared_engine(engine.clone());
                    Some(engine as Arc<dyn TaskQueue>)
                }
                Err(e) => {
                    tracing::error!(target: "fmd_server", "download engine: {e}");
                    None
                }
            };
            state = start_favorites(state, &runtime, modules, queue);
            if config.module_updates {
                module_updates::start(
                    state.clone(),
                    &runtime,
                    lua_dir.clone(),
                    flaresolverr_override.clone(),
                );
            }
        }
        Err(e) => {
            tracing::error!(target: "fmd_server", "Lua modules: {e}");
            state = state.with_covers(covers, Idle);
        }
    }
    if flaresolverr_override.is_none() {
        tokio::spawn(follow_flaresolverr_url(
            state.settings.clone(),
            lua_dir,
            flaresolverr_url,
        ));
    }
    let listener = TcpListener::bind(config.bind)
        .await
        .map_err(|source| ServeError::Bind {
            addr: config.bind,
            source,
        })?;
    let signal = shutdown_signal()?;
    let addr = listener.local_addr()?;
    tracing::info!(target: "fmd_server", "listening on {addr}");
    let shutdown = state.clone();
    axum::serve(listener, build_router(state))
        .with_graceful_shutdown(async move {
            signal.await;
            tracing::info!(target: "fmd_server", "shutting down");
            shutdown.shut_down();
        })
        .await?;
    Ok(())
}

/// Registers the `favorites` job, checking the library on `runtime`'s pool with the modules
/// `modules` finds, and starts its schedule: a check at startup and on the interval, as the
/// `favorites` settings say (`tmStartupTimer`/`tmCheckFavorites`,
/// mangadownloader/forms/frmMain.pas:1871-1878, :2078-2082). Found chapters go to `queue` when
/// `favorites.auto_download` is on and there is one, else to the inbox.
fn start_favorites(
    state: AppState,
    runtime: &LuaRuntime,
    modules: Arc<ModuleLookup>,
    queue: Option<Arc<dyn TaskQueue>>,
) -> AppState {
    let checker = FavoritesChecker::new(
        CheckerConfig {
            db: state.db.clone(),
            pool: runtime.pool.clone(),
            modules,
            settings: state.settings.clone(),
            queue,
            jobs: state.jobs.clone(),
        },
        state.favorites_events(),
    );
    state.jobs.register(checker.clone());
    state.jobs.changed(FavoritesChecker::ID);
    tokio::spawn(checker.clone().schedule());
    state.with_favorites(checker)
}

/// List updates on `runtime`'s pool and FMD2-DB imports (from the `update_lists.db_url`
/// setting) into `state`'s `lists.db`, for the module `modules` finds when each starts; their
/// events go out as `job.lists.*`. `None` without a `lists.db`.
fn list_jobs(
    state: &AppState,
    runtime: &LuaRuntime,
    modules: Arc<ModuleLookup>,
) -> Option<ListJobs> {
    let lists = state.lists.clone()?;
    let events = state.events().clone();
    Some(ListJobs::new(
        ListUpdater::new(runtime.pool.clone(), lists.clone()),
        DbImporter::new(runtime.http.clone(), lists),
        state.settings.clone(),
        move |id: &str| modules(id),
        move |event| events.publish(ServerEvent::Lists(event)),
    ))
}

/// Installs the SIGINT/SIGTERM handlers now (so no signal sent after this returns is missed) and
/// returns a future that resolves on the first of them.
#[cfg(unix)]
fn shutdown_signal() -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut int = signal(SignalKind::interrupt())?;
    let mut term = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = int.recv() => {}
            _ = term.recv() => {}
        }
    })
}

/// Resolves on Ctrl-C.
#[cfg(not(unix))]
fn shutdown_signal() -> std::io::Result<impl Future<Output = ()>> {
    Ok(async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    })
}

/// Rewrites `websitebypass_config.json` whenever the `connections.flaresolverr_url` setting
/// differs from `current`, the URL last written. Upstream's `cloudflare.lua` reads the file at every bypass
/// (lua/websitebypass/cloudflare.lua:271-325, :341), so the next one uses the new URL.
async fn follow_flaresolverr_url(
    settings: Arc<SettingsService>,
    lua_dir: PathBuf,
    mut current: String,
) {
    let mut changes = settings.subscribe();
    while changes.changed().await.is_ok() {
        let url = changes
            .borrow_and_update()
            .connections
            .flaresolverr_url
            .clone();
        if url == current {
            continue;
        }
        let dir = lua_dir.clone();
        let next = url.clone();
        let written = tokio::task::spawn_blocking(move || write_websitebypass_config(&dir, &next))
            .await
            .map_err(std::io::Error::other)
            .and_then(|written| written);
        match written {
            Ok(()) => current = url,
            // Tried again at the next settings change.
            Err(e) => {
                tracing::warn!(target: "fmd_server", "writing websitebypass_config.json: {e}")
            }
        }
    }
}
