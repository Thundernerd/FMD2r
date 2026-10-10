//! Running the server on a socket until SIGINT/SIGTERM.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use fmd_core::accounts::AccountService;
use fmd_core::download::{DownloadManager, EngineConfig, ModuleLookup};
use fmd_core::favorites::{CheckerConfig, FavoritesChecker, TaskQueue};
use fmd_core::lists::{DbImporter, ListJobs, ListUpdater};
use fmd_core::metadata::{HttpDumpSource, MangaBakaDb, MangaDexLinks, Matcher, MetadataJobs};
use fmd_core::module_updater::RepoConfig;
use fmd_core::settings::{
    ConnectionSettings, ProxyType, SettingsService, write_websitebypass_config,
};
use fmd_http::{HttpClient, Proxy, ProxyKind};
use fmd_store::{ACCOUNTS_KEY_FILE, AppDb, ListsDb};
use thiserror::Error;
use tokio::net::TcpListener;

use crate::events::ServerEvent;
use crate::lua_catalog::LuaCatalog;
use crate::module_updates::{self, LuaRuntime};
use crate::{AppState, CoverConfig, Idle, LogBuffer, LogRotation, SystemTools, build_router};

pub struct ServeConfig {
    /// `--bind` / `FMD2R_BIND`; `None` takes the `server.bind` setting.
    pub bind: Option<SocketAddr>,
    /// Holds the databases, `lua/`, `covers/` and `logs/`; created when missing.
    pub data_dir: PathBuf,
    /// Password/token required for the API; `None` leaves it open.
    pub auth: Option<String>,
    /// Overrides `connections.flaresolverr_url` for this run without changing the setting.
    pub flaresolverr_url: Option<String>,
    /// The buffer the `tracing` subscriber feeds; persisted to `<data dir>/logs/`.
    pub logs: LogBuffer,
    /// Run the module updater. Off, nothing is fetched from GitHub; the modules already in
    /// `<data dir>/lua` still load.
    pub module_updates: bool,
}

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
    #[error("the server.bind setting {value:?}: {source}")]
    BindSetting {
        value: String,
        source: std::net::AddrParseError,
    },
    #[error("bind {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("server: {0}")]
    Io(#[from] std::io::Error),
}

/// Serves until SIGINT or SIGTERM, letting in-flight requests finish and closing event streams.
pub async fn serve(config: ServeConfig) -> Result<(), ServeError> {
    std::fs::create_dir_all(&config.data_dir).map_err(|source| ServeError::DataDir {
        path: config.data_dir.clone(),
        source,
    })?;
    let db_path = config.data_dir.join("app.db");
    let lists_path = config.data_dir.join("lists.db");
    // Opening the stores and loading the settings blocks.
    let state = tokio::task::spawn_blocking(move || -> Result<AppState, ServeError> {
        let lists = ListsDb::open(lists_path)?;
        let state = AppState::new(AppDb::open(db_path)?)?;
        // Before anything else stores settings, which would make a fresh install look set up.
        if let Err(e) = state.settings.mark_existing_install_set_up(&lists) {
            tracing::warn!(target: "fmd_server", "deciding whether setup is needed: {e}");
        }
        if let Err(e) = state.settings.select_listed_websites(&lists) {
            tracing::warn!(target: "fmd_server", "selecting the websites with a list: {e}");
        }
        Ok(state.with_lists(lists))
    })
    .await
    .map_err(std::io::Error::other)??;
    // Read once: cover cache and log rotation changes apply on the next start.
    let settings = state.settings.get();
    // Absolute, so `GET /api/about` shows where the data really is.
    let data_dir = std::fs::canonicalize(&config.data_dir).unwrap_or(config.data_dir);
    let lua_dir = data_dir.join("lua");
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
    let bind = match config.bind {
        Some(bind) => bind,
        None => settings
            .server
            .bind
            .parse()
            .map_err(|source| ServeError::BindSetting {
                value: settings.server.bind.clone(),
                source,
            })?,
    };
    let overridden = [
        config.bind.map(|_| "server.bind"),
        flaresolverr_override
            .as_ref()
            .map(|_| "connections.flaresolverr_url"),
    ];
    let mut state = state
        .with_logs(config.logs)
        .with_data_dir(&data_dir)
        .with_tools(SystemTools::new(bypass_config))
        .with_listen_addr(bind)
        .with_overridden(overridden.into_iter().flatten());
    if let Some(secret) = config.auth {
        state = state.with_auth(secret);
    }
    // Loading the modules blocks.
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
            // Before the download engine resumes anything.
            follow_connections(state.settings.clone(), runtime.http.clone());
            module_updates::follow_xpath_backend(state.settings.clone(), &runtime);
            let upstream_ref = RepoConfig::from_settings(&settings.module_updater).git_ref;
            let catalog = LuaCatalog::new(&runtime, state.db.clone(), &lua_dir, upstream_ref);
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
            if let Some(jobs) = metadata_jobs(&state, &runtime, &data_dir, modules.clone()) {
                jobs.register(&state.jobs);
                tokio::spawn(jobs.clone().schedule());
                if let Some(lists) = &state.list_jobs {
                    let matching = jobs.clone();
                    lists
                        .on_list_changed(move |id, terminate| matching.list_changed(id, terminate));
                }
                state = state.with_metadata(jobs);
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
    if !bind.ip().is_loopback() && state.secret().is_none() {
        tracing::warn!(target: "fmd_server",
            "{bind} is reachable from other machines and no password is set: anyone who can reach \
             it can use the API. Set one in the settings (Server, Password) or with --password / \
             FMD2R_PASSWORD");
    }
    let listener = TcpListener::bind(bind)
        .await
        .map_err(|source| ServeError::Bind { addr: bind, source })?;
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

/// Registers the `favorites` job and starts its schedule (`tmStartupTimer`/`tmCheckFavorites`,
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

/// List updates and FMD2-DB imports into `lists.db`; `None` without one.
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

/// The MangaBaka database, matched against `lists.db`; `None` without one.
fn metadata_jobs(
    state: &AppState,
    runtime: &LuaRuntime,
    data_dir: &std::path::Path,
    modules: Arc<ModuleLookup>,
) -> Option<MetadataJobs> {
    let lists = state.lists.clone()?;
    let source = HttpDumpSource::new(state.settings.clone());
    let db = Arc::new(MangaBakaDb::open(data_dir, Arc::new(source)));
    let matcher = Matcher::new(lists.clone(), MangaDexLinks::new(runtime.http.clone()));
    Some(MetadataJobs::new(
        db,
        matcher,
        lists,
        move |id: &str| modules(id).map(|m| m.def().root_url),
        state.settings.clone(),
        state.metadata_events(),
    ))
}

/// Installs the handlers now, so no signal sent after this returns is missed.
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

/// Rewrites `websitebypass_config.json` when `connections.flaresolverr_url` changes;
/// `cloudflare.lua` reads it at every bypass (lua/websitebypass/cloudflare.lua:271-325, :341).
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

fn follow_connections(settings: Arc<SettingsService>, http: HttpClient) {
    let mut changes = settings.subscribe();
    let mut current = changes.borrow_and_update().connections.clone();
    apply_connections(&http, &current);
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            let next = changes.borrow_and_update().connections.clone();
            if next != current {
                apply_connections(&http, &next);
                current = next;
            }
        }
    });
}

/// As `ApplyOptions` (mangadownloader/forms/frmMain.pas:6264-6295): retry count, timeout and proxy
/// for new and existing sessions (`Set…AndApply`, baseunits/httpsendthread.pas:332-392), the user
/// agent for new sessions only (`DefaultUserAgent`).
fn apply_connections(http: &HttpClient, connections: &ConnectionSettings) {
    http.set_default_user_agent(connections.user_agent.clone());
    http.set_default_retry_count(connections.retry_count);
    http.set_default_timeout(connections.timeout_secs.saturating_mul(1000));
    http.set_default_proxy(global_proxy(connections));
}

/// The global proxy; `None` when it is off (`SetDefaultProxyAndApply('', …)`,
/// mangadownloader/forms/frmMain.pas:6295).
fn global_proxy(connections: &ConnectionSettings) -> Option<Proxy> {
    let proxy = &connections.proxy;
    if !proxy.enabled {
        return None;
    }
    let kind = match proxy.kind {
        ProxyType::Http => ProxyKind::Http,
        ProxyType::Socks4 => ProxyKind::Socks4,
        ProxyType::Socks5 => ProxyKind::Socks5,
    };
    Some(Proxy {
        kind,
        host: proxy.host.clone(),
        port: proxy.port.map(|p| p.to_string()).unwrap_or_default(),
        user: proxy.username.clone(),
        pass: proxy.password.clone(),
    })
}
