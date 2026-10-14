//! Asking the comic server (`comic_server` settings) to scan after downloads finish, and
//! `POST /api/settings/comic-server/test` (issue #269). No FMD2 counterpart.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use fmd_core::comic_server::{ComicServer, ComicServerClient, Connection, Debounce, Library};
use fmd_core::download::{EngineEvent, TaskStatus};
use fmd_core::settings::{ComicServerKind, ComicServerSettings};
use fmd_store::{EventSeverity, NewEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::{ApiError, AppState, Problem};

/// The `events.kind` of a failed scan's inbox item.
const EVENT_KIND: &str = "comic_server";

impl AppState {
    /// Asks the comic server to scan `window` after the last download finished or move ended
    /// ([`AppState::request_comic_server_scan`]), so a batch of them gives one scan. Failed or
    /// stopped downloads don't count. A failed scan logs a warning and adds a warning to the
    /// inbox; it never affects downloads. Call it once, after [`AppState::with_engine`].
    pub fn start_comic_server_scans(&self, window: Duration) {
        let client = match ComicServerClient::new() {
            Ok(client) => client,
            Err(e) => {
                tracing::warn!(target: "fmd_server", "comic server scans are off: {e}");
                return;
            }
        };
        let state = self.clone();
        let debounce = Debounce::spawn(window, move || {
            let (state, client) = (state.clone(), client.clone());
            async move { scan(&state, &client).await }
        });
        if self.comic_server_scans.set(debounce).is_err() {
            tracing::warn!(target: "fmd_server", "comic server scans were already started");
            return;
        }
        let Some(mut rx) = self.engine.subscribe() else {
            return;
        };
        let state = self.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(EngineEvent::Status {
                        status: TaskStatus::Finished,
                        ..
                    }) => state.request_comic_server_scan(),
                    // Missed events may have held a finish; a spare scan does no harm.
                    Err(RecvError::Lagged(_)) => state.request_comic_server_scan(),
                    Ok(_) => {}
                    Err(RecvError::Closed) => return,
                }
            }
        });
    }

    /// Asks for a comic server scan once the scan delay passes without another request; nothing
    /// without [`AppState::start_comic_server_scans`].
    pub(crate) fn request_comic_server_scan(&self) {
        if let Some(debounce) = self.comic_server_scans.get() {
            debounce.trigger();
        }
    }
}

/// Asks the server the settings name, if any, to scan.
async fn scan(state: &AppState, client: &ComicServerClient) {
    let settings = state.settings.get();
    let Some(server) = ComicServer::from_settings(&settings.comic_server) else {
        return;
    };
    let Err(e) = client.scan(&server).await else {
        tracing::info!(target: "fmd_server", "asked {} to scan", server.name());
        return;
    };
    let title = format!("Could not ask {} to scan", server.name());
    tracing::warn!(target: "fmd_server", "{title}: {e}");
    let event = NewEvent {
        kind: EVENT_KIND.into(),
        severity: EventSeverity::Warning,
        module_id: None,
        task_id: None,
        title,
        body: e.to_string().into(),
    };
    if let Err(e) = state.notify(event).await {
        tracing::warn!(target: "fmd_server", "adding the comic server warning to the inbox: {e}");
    }
}

/// The comic server to test: the `comic_server` settings as the form holds them, saved or not.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ComicServerTestRequest {
    pub kind: ComicServerKind,
    pub url: String,
    /// A key typed but not saved yet; without one, the stored key, if `kind` and `url` are the
    /// stored ones (so the stored key goes nowhere else).
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub library_id: Option<String>,
}

/// What "Test connection" found.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ComicServerTest {
    /// The server answered and accepted the API key.
    pub ok: bool,
    /// What happened, as a sentence to show.
    pub message: String,
    /// Komga: the libraries the API key can see. Empty for Kavita or on failure.
    pub libraries: Vec<Library>,
}

impl ComicServerTest {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            libraries: Vec::new(),
        }
    }
}

/// Check a comic server's URL and API key with a request that changes nothing: Komga's library
/// list, Kavita's API key expiry.
#[utoipa::path(post, path = "/api/settings/comic-server/test", tag = "settings",
    operation_id = "testComicServer",
    request_body(content = ComicServerTestRequest, content_type = "application/json"),
    responses(
        (status = 200, body = ComicServerTest, description = "Whether the server answered"),
        (status = 400, description = "Malformed body", body = Problem),
    ))]
pub(crate) async fn test(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<ComicServerTestRequest>,
) -> Result<Json<ComicServerTest>, ApiError> {
    let stored = state.settings.get().comic_server.clone();
    let same_server = stored.kind == req.kind && stored.url == req.url.trim();
    let api_key = match req.api_key.filter(|key| !key.is_empty()) {
        Some(typed) => Some(typed),
        None if same_server => stored.api_key,
        None => None,
    };
    let settings = ComicServerSettings {
        kind: req.kind,
        url: req.url,
        api_key,
        library_id: req.library_id,
    };
    let Some(server) = ComicServer::from_settings(&settings) else {
        return Ok(Json(ComicServerTest::failed(
            "Choose Komga or Kavita and enter its URL first.",
        )));
    };
    let client = ComicServerClient::new().map_err(|e| ApiError::Internal(e.to_string()))?;
    let result = match client.test(&server).await {
        Ok(Connection::Komga { libraries }) => {
            let names: Vec<&str> = libraries.iter().map(|l| l.name.as_str()).collect();
            let message = if names.is_empty() {
                "Connected to Komga, but the API key sees no libraries.".to_string()
            } else {
                format!("Connected to Komga. Libraries: {}.", names.join(", "))
            };
            ComicServerTest {
                ok: true,
                message,
                libraries,
            }
        }
        Ok(Connection::Kavita { key_expires }) => ComicServerTest {
            ok: true,
            message: match key_expires {
                Some(date) => format!("Connected to Kavita. The API key expires {date}."),
                None => "Connected to Kavita. The API key does not expire.".to_string(),
            },
            libraries: Vec::new(),
        },
        Err(e) => ComicServerTest::failed(format!("Could not connect to {}: {e}.", server.name())),
    };
    Ok(Json(result))
}
