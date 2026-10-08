//! Running the server on a socket until SIGINT/SIGTERM.

use std::net::SocketAddr;
use std::path::PathBuf;

use fmd_store::AppDb;
use thiserror::Error;
use tokio::net::TcpListener;

use crate::{AppState, CoverConfig, Idle, LogBuffer, SystemTools, build_router};

/// What [`serve`] needs.
pub struct ServeConfig {
    pub bind: SocketAddr,
    /// Holds `app.db`, the Lua tree (`lua/`) and the cover cache (`covers/`); created when missing.
    pub data_dir: PathBuf,
    /// Password/token required for the API; `None` leaves it open.
    pub auth: Option<String>,
    /// The buffer the `tracing` subscriber feeds; `GET /api/logs` reads it and `GET /api/events`
    /// streams its bus.
    pub logs: LogBuffer,
}

/// Errors that stop [`serve`].
#[derive(Debug, Error)]
pub enum ServeError {
    #[error("data dir {path}: {source}")]
    DataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("app.db: {0}")]
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
    // Opening the store and loading the settings block.
    let state = tokio::task::spawn_blocking(move || -> Result<AppState, ServeError> {
        Ok(AppState::new(AppDb::open(db_path)?)?)
    })
    .await
    .map_err(std::io::Error::other)??;
    // Absolute, so `GET /api/about` shows where the data really is.
    let data_dir = std::fs::canonicalize(&config.data_dir).unwrap_or(config.data_dir);
    let bypass_config = data_dir.join("lua/websitebypass/websitebypass_config.json");
    // Read once: cover cache changes apply on the next start.
    let covers = CoverConfig::from_settings(data_dir.join("covers"), &state.settings.get().covers);
    let mut state = state
        .with_logs(config.logs)
        .with_data_dir(&data_dir)
        .with_tools(SystemTools::new(bypass_config))
        .with_covers(covers, Idle);
    if let Some(secret) = config.auth {
        state = state.with_auth(secret);
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
