//! Shared state handed to every handler.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use fmd_core::jobs::JobRegistry;
use fmd_store::{AppDb, NewEvent};
use tokio::sync::watch;

use crate::ApiError;
use crate::auth::Auth;
use crate::events::{EventBus, ServerEvent};
use crate::inbox::InboxItem;
use crate::logs::LogBuffer;
use crate::services::{DownloadEngine, Idle, ModuleCatalog};
use crate::settings::{SettingsService, StoreSettings};
use crate::spa::{Assets, EmbeddedAssets};
use crate::tools::{SystemTools, ToolProbe};

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
    pub(crate) settings: Arc<dyn SettingsService>,
    pub(crate) engine: Arc<dyn DownloadEngine>,
    pub(crate) jobs: JobRegistry,
    pub(crate) modules: Arc<dyn ModuleCatalog>,
    pub(crate) tools: Arc<dyn ToolProbe>,
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) started: Instant,
    pub(crate) shutdown: Arc<watch::Sender<bool>>,
}

impl AppState {
    /// State backed by `db`, serving the embedded web UI, with untyped store settings, an idle
    /// engine, no jobs or modules, real tool checks and no auth configured.
    pub fn new(db: AppDb) -> Self {
        let events = EventBus::new();
        Self {
            logs: LogBuffer::new(DEFAULT_LOG_LINES, events.clone()),
            settings: Arc::new(StoreSettings::new(db.clone())),
            engine: Arc::new(Idle),
            jobs: JobRegistry::new(),
            modules: Arc::new(Idle),
            tools: Arc::new(SystemTools::default()),
            data_dir: None,
            started: Instant::now(),
            shutdown: Arc::new(watch::channel(false).0),
            db,
            assets: Arc::new(EmbeddedAssets),
            auth: None,
            events,
        }
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

    /// Serves `/api/settings` from `settings`.
    pub fn with_settings(mut self, settings: impl SettingsService) -> Self {
        self.settings = Arc::new(settings);
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

    /// Checks external tools with `tools` for `GET /api/about`.
    pub fn with_tools(mut self, tools: impl ToolProbe) -> Self {
        self.tools = Arc::new(tools);
        self
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
        tokio::task::spawn_blocking(move || f(&db).map_err(Into::into))
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
    }
}
