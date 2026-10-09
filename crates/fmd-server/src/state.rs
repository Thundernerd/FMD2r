//! Shared state handed to every handler.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use fmd_core::accounts::AccountService;
use fmd_core::favorites::CheckerEvent;
use fmd_core::jobs::JobRegistry;
use fmd_core::lists::ListJobs;
use fmd_core::metadata::{MetadataEvent, MetadataJobs};
use fmd_core::settings::{SettingsError, SettingsService};
use fmd_store::{AppDb, ListsDb, NewEvent};
use tokio::sync::watch;

use crate::ApiError;
use crate::auth::{Auth, Secret};
use crate::covers::{CoverConfig, CoverModules, Covers};
use crate::events::{EventBus, ServerEvent};
use crate::import::{ImportJob, ImportLimits};
use crate::inbox::InboxItem;
use crate::logs::LogBuffer;
use crate::series::InfoCache;
use crate::services::{DownloadEngine, FavoritesJobs, Idle, ModuleCatalog};
use crate::spa::{Assets, EmbeddedAssets};
use crate::tools::{NoTools, ToolProbe};

/// Log lines kept when no buffer is supplied.
const DEFAULT_LOG_LINES: usize = 1000;

/// Everything the handlers share. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub(crate) db: AppDb,
    pub(crate) assets: Arc<dyn Assets>,
    pub(crate) auth: Arc<Auth>,
    /// Where the server listens, for `GET /api/health`.
    pub(crate) listen_addr: Option<SocketAddr>,
    /// The settings the command line or environment overrides, as dotted paths.
    pub(crate) overridden: Vec<&'static str>,
    pub(crate) events: EventBus,
    pub(crate) logs: LogBuffer,
    pub(crate) settings: Arc<SettingsService>,
    pub(crate) engine: Arc<dyn DownloadEngine>,
    pub(crate) jobs: JobRegistry,
    pub(crate) modules: Arc<dyn ModuleCatalog>,
    pub(crate) series_cache: Arc<InfoCache>,
    pub(crate) accounts: Option<Arc<AccountService>>,
    pub(crate) tools: Arc<dyn ToolProbe>,
    pub(crate) covers: Option<Arc<Covers>>,
    pub(crate) lists: Option<ListsDb>,
    pub(crate) list_jobs: Option<ListJobs>,
    pub(crate) favorites: Option<Arc<dyn FavoritesJobs>>,
    pub(crate) metadata: Option<MetadataJobs>,
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) import_limits: ImportLimits,
    pub(crate) import_job: ImportJob,
    pub(crate) started: Instant,
    pub(crate) clock: Arc<dyn Fn() -> SystemTime + Send + Sync>,
    pub(crate) shutdown: Arc<watch::Sender<bool>>,
    /// Bumped whenever login sessions are ended, so open event streams close and reconnect
    /// through the auth check.
    pub(crate) sessions_ended: Arc<watch::Sender<u64>>,
}

impl AppState {
    /// State backed by `db`, serving the embedded web UI, with the settings stored in `db`, an
    /// idle engine, no jobs, modules, accounts, covers or tool checks, and no auth configured.
    pub fn new(db: AppDb) -> Result<Self, SettingsError> {
        let events = EventBus::new();
        Ok(Self {
            logs: LogBuffer::new(DEFAULT_LOG_LINES, events.clone()),
            settings: Arc::new(SettingsService::load(db.clone())?),
            engine: Arc::new(Idle),
            jobs: JobRegistry::new(),
            modules: Arc::new(Idle),
            series_cache: Arc::default(),
            accounts: None,
            tools: Arc::new(NoTools),
            covers: None,
            lists: None,
            list_jobs: None,
            favorites: None,
            metadata: None,
            data_dir: None,
            import_limits: ImportLimits::default(),
            import_job: ImportJob::default(),
            started: Instant::now(),
            clock: Arc::new(SystemTime::now),
            shutdown: Arc::new(watch::channel(false).0),
            sessions_ended: Arc::new(watch::channel(0).0),
            db,
            assets: Arc::new(EmbeddedAssets),
            auth: Auth::from_settings(),
            listen_addr: None,
            overridden: Vec::new(),
            events,
        })
    }

    /// Serves the web UI from `assets` instead of the embedded `web/build`.
    pub fn with_assets(mut self, assets: impl Assets) -> Self {
        self.assets = Arc::new(assets);
        self
    }

    /// Requires `secret` (as a bearer token, or via a `POST /api/login` session) for every API
    /// route except health, login and the OpenAPI document, instead of the `server.auth_token`
    /// setting: the password given on the command line or in the environment.
    pub fn with_auth(mut self, secret: impl Into<String>) -> Self {
        self.auth = Auth::fixed(secret.into());
        self
    }

    /// Reports in `GET /api/health` whether `addr`, where the server listens, is a loopback
    /// address. Without it the server counts as loopback-only.
    pub fn with_listen_addr(mut self, addr: SocketAddr) -> Self {
        self.listen_addr = Some(addr);
        self
    }

    /// Reports in `GET /api/health` that the command line or environment overrides `settings`
    /// (dotted paths such as `server.bind`). [`AppState::with_auth`] reports `server.auth_token`
    /// itself.
    pub fn with_overridden(mut self, settings: impl IntoIterator<Item = &'static str>) -> Self {
        self.overridden.extend(settings);
        self
    }

    /// The password requests need now, or `None` when the API is open.
    pub(crate) fn secret(&self) -> Option<Secret> {
        self.auth.current(&self.settings.get())
    }

    /// Reads the wall-clock time from `clock` instead of the system clock (login sessions
    /// expire by it).
    pub fn with_clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// The current wall-clock time.
    pub(crate) fn now(&self) -> SystemTime {
        (self.clock)()
    }

    /// Serves `GET /api/logs` from `logs` (the buffer installed as a `tracing` layer) and streams
    /// `GET /api/events` from the bus `logs` publishes to.
    pub fn with_logs(mut self, logs: LogBuffer) -> Self {
        self.events = logs.events().clone();
        self.logs = logs;
        self
    }

    /// Serves `/api/settings` from `settings`, shared with whoever else subscribes to it.
    pub fn with_settings(mut self, settings: Arc<SettingsService>) -> Self {
        self.settings = settings;
        self
    }

    pub fn with_engine(self, engine: impl DownloadEngine) -> Self {
        self.with_shared_engine(Arc::new(engine))
    }

    /// [`AppState::with_engine`] for an engine others hold too.
    pub(crate) fn with_shared_engine(mut self, engine: Arc<dyn DownloadEngine>) -> Self {
        self.engine = engine;
        self
    }

    /// Lists and controls the jobs in `jobs` via `/api/jobs`, and streams their changes as
    /// `job.state` events.
    pub fn with_jobs(mut self, jobs: JobRegistry) -> Self {
        self.jobs = jobs;
        self
    }

    /// Reports the Lua modules from `modules` in `GET /api/about`, and serves `/api/resolve` and
    /// `/api/series` from them.
    pub fn with_modules(mut self, modules: impl ModuleCatalog) -> Self {
        self.modules = Arc::new(modules);
        self.series_cache = Arc::default();
        self
    }

    /// Serves `/api/accounts` from `accounts`, and streams their status changes as
    /// `account.state` events. Without it no module has an account.
    pub fn with_accounts(mut self, accounts: Arc<AccountService>) -> Self {
        self.accounts = Some(accounts);
        self
    }

    /// Checks external tools with `tools` for `GET /api/about`.
    pub fn with_tools(mut self, tools: impl ToolProbe) -> Self {
        self.tools = Arc::new(tools);
        self
    }

    /// Serves `GET /api/covers`, fetching covers through `modules` and caching them as `config`
    /// says. Without it, every cover is a 404.
    pub fn with_covers(mut self, config: CoverConfig, modules: impl CoverModules) -> Self {
        self.covers = Some(Arc::new(Covers::new(config, Arc::new(modules))));
        self
    }

    /// Serves the Discover endpoints (`/api/lists/search`, `/api/lists/facets`) and the list
    /// sizes in `GET /api/modules` from `lists`. Without it they answer 503.
    pub fn with_lists(mut self, lists: ListsDb) -> Self {
        self.lists = Some(lists);
        self
    }

    /// Starts list updates and FMD2-DB imports (`POST /api/lists/{module}/...`) with `jobs`.
    /// Without it they answer 503. `jobs` should send its events to [`AppState::events`] as
    /// [`ServerEvent::Lists`].
    pub fn with_list_jobs(mut self, jobs: ListJobs) -> Self {
        self.list_jobs = Some(jobs);
        self
    }

    /// Starts favorites checks (`POST /api/favorites/check`, `.../check-missing`) with `jobs`.
    /// Without it they answer 503. A [`fmd_core::favorites::FavoritesChecker`] should send its
    /// events to [`AppState::favorites_events`].
    pub fn with_favorites(mut self, jobs: impl FavoritesJobs) -> Self {
        self.favorites = Some(Arc::new(jobs));
        self
    }

    /// Where a [`fmd_core::favorites::FavoritesChecker`] sends its events: its progress goes out
    /// as `job.favorites.<kind>`, its inbox items as `inbox.new`.
    pub fn favorites_events(&self) -> impl Fn(CheckerEvent) + Send + Sync + 'static + use<> {
        let events = self.events.clone();
        move |event| match event {
            CheckerEvent::Job(e) => events.publish(ServerEvent::Favorites(e)),
            CheckerEvent::Inbox(e) => events.publish(ServerEvent::InboxNew(InboxItem::from(e))),
        }
    }

    /// Serves `/api/metadata/mangabaka` (the MangaBaka database's download, status and removal)
    /// with `jobs`, and the list titles' MangaBaka metadata on the series page. Without it those
    /// answer as if no database was downloaded. `jobs` should send its events to
    /// [`AppState::metadata_events`].
    pub fn with_metadata(mut self, jobs: MetadataJobs) -> Self {
        self.metadata = Some(jobs);
        self
    }

    /// Where [`MetadataJobs`] send their events: out as `job.metadata.<kind>`.
    pub fn metadata_events(&self) -> impl Fn(MetadataEvent) + Send + Sync + 'static + use<> {
        let events = self.events.clone();
        move |event| events.publish(ServerEvent::Metadata(event))
    }

    /// The settings the handlers read and change.
    pub fn settings(&self) -> &Arc<SettingsService> {
        &self.settings
    }

    /// The `lists.db` behind the Discover endpoints, or a 503.
    pub(crate) fn lists(&self) -> Result<ListsDb, ApiError> {
        self.lists
            .clone()
            .ok_or_else(|| ApiError::Unavailable("lists.db is not open".into()))
    }

    /// Reports `dir` and the sizes of the databases in it in `GET /api/about`.
    pub fn with_data_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.data_dir = Some(dir.as_ref().to_owned());
        self
    }

    /// Caps the size of `POST /api/import` uploads (instead of [`ImportLimits::default`]).
    pub fn with_import_limits(mut self, limits: ImportLimits) -> Self {
        self.import_limits = limits;
        self
    }

    /// The bus behind `GET /api/events`.
    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// Ends every open event stream so a graceful shutdown is not held up by SSE clients.
    pub fn shut_down(&self) {
        self.shutdown.send_replace(true);
    }

    /// Resolves once [`AppState::shut_down`] has been called.
    pub(crate) fn shutting_down(&self) -> impl Future<Output = ()> + Send + use<> {
        let mut rx = self.shutdown.subscribe();
        async move {
            // An error means the sender is gone, which also means shutdown.
            let _ = rx.wait_for(|down| *down).await;
        }
    }

    /// Closes every open event stream because login sessions ended; clients reconnect, and
    /// those whose session is gone get a 401.
    pub(crate) fn end_sessions(&self) {
        self.sessions_ended.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Resolves once the server shuts down or [`AppState::end_sessions`] is called.
    pub(crate) fn stream_ended(&self) -> impl Future<Output = ()> + Send + use<> {
        let shutdown = self.shutting_down();
        let mut ended = self.sessions_ended.subscribe();
        async move {
            let ended = async move {
                // An error means the sender is gone, which also ends the stream.
                let _ = ended.changed().await;
            };
            futures_util::future::select(Box::pin(shutdown), Box::pin(ended)).await;
        }
    }

    /// Stores `event` in the inbox and announces it as `inbox.new`
    /// (FMD2's balloon hints, baseunits/uDownloadsManager.pas:741-803).
    pub async fn notify(&self, event: NewEvent) -> Result<InboxItem, ApiError> {
        let stored = self.blocking(move |db| db.events().push(&event)).await?;
        let item = InboxItem::from(stored);
        self.events.publish(ServerEvent::InboxNew(item.clone()));
        Ok(item)
    }

    /// The folder the module's downloads go to when the user picks none; empty for the default
    /// destination (its `OverrideSettings.SaveToPath`, T74).
    pub(crate) async fn website_dir(&self, module_id: &str) -> Result<String, ApiError> {
        let id = module_id.to_owned();
        self.blocking(move |db| -> Result<String, ApiError> {
            Ok(db
                .module_settings()
                .get(&id)?
                .map(|stored| stored.save_to)
                .unwrap_or_default())
        })
        .await
    }

    /// Runs blocking store work on the blocking thread pool.
    pub(crate) async fn blocking<T, E>(
        &self,
        f: impl FnOnce(&AppDb) -> Result<T, E> + Send + 'static,
    ) -> Result<T, ApiError>
    where
        T: Send + 'static,
        E: Into<ApiError> + Send + 'static,
    {
        let db = self.db.clone();
        off_thread(move || f(&db).map_err(Into::into)).await?
    }
}

/// Runs blocking work (store calls, job control, tool checks) on the blocking thread pool.
pub(crate) async fn off_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))
}
