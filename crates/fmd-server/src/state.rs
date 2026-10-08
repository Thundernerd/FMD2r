//! Shared state handed to every handler.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use fmd_core::accounts::AccountService;
use fmd_core::jobs::JobRegistry;
use fmd_core::lists::ListJobs;
use fmd_core::settings::{SettingsError, SettingsService};
use fmd_store::{AppDb, ListsDb, NewEvent};
use tokio::sync::watch;

use crate::ApiError;
use crate::auth::Auth;
use crate::covers::{CoverConfig, CoverModules, Covers};
use crate::events::{EventBus, ServerEvent};
use crate::inbox::InboxItem;
use crate::logs::LogBuffer;
use crate::services::{DownloadEngine, Idle, ModuleCatalog};
use crate::spa::{Assets, EmbeddedAssets};
use crate::tools::{NoTools, ToolProbe};

/// Log lines kept when no buffer is supplied.
const DEFAULT_LOG_LINES: usize = 1000;

/// Everything the handlers share. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub(crate) db: AppDb,
    pub(crate) assets: Arc<dyn Assets>,
    pub(crate) auth: Option<Arc<Auth>>,
    pub(crate) events: EventBus,
    pub(crate) logs: LogBuffer,
    pub(crate) settings: Arc<SettingsService>,
    pub(crate) engine: Arc<dyn DownloadEngine>,
    pub(crate) jobs: JobRegistry,
    pub(crate) modules: Arc<dyn ModuleCatalog>,
    pub(crate) accounts: Option<Arc<AccountService>>,
    pub(crate) tools: Arc<dyn ToolProbe>,
    pub(crate) covers: Option<Arc<Covers>>,
    pub(crate) lists: Option<ListsDb>,
    pub(crate) list_jobs: Option<ListJobs>,
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) started: Instant,
    pub(crate) shutdown: Arc<watch::Sender<bool>>,
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
            accounts: None,
            tools: Arc::new(NoTools),
            covers: None,
            lists: None,
            list_jobs: None,
            data_dir: None,
            started: Instant::now(),
            shutdown: Arc::new(watch::channel(false).0),
            db,
            assets: Arc::new(EmbeddedAssets),
            auth: None,
            events,
        })
    }

    /// Serves the web UI from `assets` instead of the embedded `web/build`.
    pub fn with_assets(mut self, assets: impl Assets) -> Self {
        self.assets = Arc::new(assets);
        self
    }

    /// Requires `secret` (as a bearer token, or via a `POST /api/login` session) for every API
    /// route except health, login and the OpenAPI document.
    pub fn with_auth(mut self, secret: impl Into<String>) -> Self {
        self.auth = Some(Auth::new(secret.into()));
        self
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

    pub fn with_engine(mut self, engine: impl DownloadEngine) -> Self {
        self.engine = Arc::new(engine);
        self
    }

    /// Lists and controls the jobs in `jobs` via `/api/jobs`, and streams their changes as
    /// `job.state` events.
    pub fn with_jobs(mut self, jobs: JobRegistry) -> Self {
        self.jobs = jobs;
        self
    }

    /// Reports the Lua modules from `modules` in `GET /api/about`.
    pub fn with_modules(mut self, modules: impl ModuleCatalog) -> Self {
        self.modules = Arc::new(modules);
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

    /// Stores `event` in the inbox and announces it as `inbox.new`
    /// (FMD2's balloon hints, baseunits/uDownloadsManager.pas:741-803).
    pub async fn notify(&self, event: NewEvent) -> Result<InboxItem, ApiError> {
        let stored = self.blocking(move |db| db.events().push(&event)).await?;
        let item = InboxItem::from(stored);
        self.events.publish(ServerEvent::InboxNew(item.clone()));
        Ok(item)
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
