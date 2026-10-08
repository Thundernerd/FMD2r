//! Shared state handed to every handler.

use std::sync::Arc;

use fmd_store::{AppDb, NewEvent};
use tokio::sync::watch;

use crate::ApiError;
use crate::auth::Auth;
use crate::events::{EventBus, ServerEvent};
use crate::inbox::InboxItem;
use crate::logs::LogBuffer;
use crate::services::{DownloadEngine, Idle, Jobs};
use crate::settings::{SettingsService, StoreSettings};
use crate::spa::{Assets, EmbeddedAssets};

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
    pub(crate) jobs: Arc<dyn Jobs>,
    pub(crate) shutdown: Arc<watch::Sender<bool>>,
}

impl AppState {
    /// State backed by `db`, serving the embedded web UI, with untyped store settings, an idle
    /// engine and no auth configured.
    pub fn new(db: AppDb) -> Self {
        let events = EventBus::new();
        Self {
            logs: LogBuffer::new(DEFAULT_LOG_LINES, events.clone()),
            settings: Arc::new(StoreSettings::new(db.clone())),
            engine: Arc::new(Idle),
            jobs: Arc::new(Idle),
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

    /// Publishes to `bus` instead of a bus of its own (e.g. one a log layer also feeds).
    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.events = bus;
        self
    }

    /// Serves `GET /api/logs` from `logs` (the buffer installed as a `tracing` layer).
    pub fn with_logs(mut self, logs: LogBuffer) -> Self {
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

    pub fn with_jobs(mut self, jobs: impl Jobs) -> Self {
        self.jobs = Arc::new(jobs);
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
