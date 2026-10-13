//! Asking a comic server (Komga, Kavita) to scan its libraries, so new downloads show up before
//! its next scheduled scan (issue #269). No FMD2 counterpart.

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use utoipa::ToSchema;

use crate::settings::{ComicServerKind, ComicServerSettings};

/// How long after the last download finished the comic server is asked to scan, so a batch of
/// downloads gives one scan.
pub const SCAN_DELAY: Duration = Duration::from_secs(60);

/// How long one request to the comic server may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum ComicServerError {
    /// The server answered with a status other than 2xx.
    #[error("the server answered {0}")]
    Status(u16),
    /// No answer: the server is unreachable, the URL is wrong, or it timed out.
    #[error("{0}")]
    Request(String),
    /// An answer that is not what the API documents.
    #[error("unexpected answer: {0}")]
    Body(String),
}

/// The methods the comic server requests use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

/// One request to a comic server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

/// A configured comic server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComicServer {
    kind: Kind,
    /// The base URL without a trailing `/`.
    url: String,
    api_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Komga { library_id: Option<String> },
    Kavita,
}

impl ComicServer {
    /// `None` when no server is set up: kind `none` or no URL.
    pub fn from_settings(settings: &ComicServerSettings) -> Option<Self> {
        let kind = match settings.kind {
            ComicServerKind::None => return None,
            ComicServerKind::Komga => Kind::Komga {
                library_id: settings.library_id.clone().filter(|id| !id.is_empty()),
            },
            ComicServerKind::Kavita => Kind::Kavita,
        };
        let url = settings.url.trim().trim_end_matches('/');
        if url.is_empty() {
            return None;
        }
        Some(Self {
            kind,
            url: url.to_string(),
            api_key: settings.api_key.clone().unwrap_or_default(),
        })
    }

    /// The scan requests: Komga's one library, else each of `libraries` (the IDs
    /// [`Self::libraries_request`] answered); Kavita's scan of every library.
    pub fn scan_requests(&self, libraries: &[String]) -> Vec<ApiRequest> {
        match &self.kind {
            Kind::Komga { library_id } => {
                let ids = library_id.as_slice();
                let ids = if ids.is_empty() { libraries } else { ids };
                ids.iter()
                    .map(|id| {
                        self.request(HttpMethod::Post, &format!("/api/v1/libraries/{id}/scan"))
                    })
                    .collect()
            }
            Kind::Kavita => vec![self.request(HttpMethod::Post, "/api/Library/scan-all")],
        }
    }

    /// Komga without a library ID: the request listing the libraries to scan
    /// (`GET /api/v1/libraries`); `None` otherwise.
    pub fn libraries_request(&self) -> Option<ApiRequest> {
        match &self.kind {
            Kind::Komga { library_id: None } => Some(self.komga_libraries()),
            Kind::Komga { .. } | Kind::Kavita => None,
        }
    }

    /// The "Test connection" request: Komga's library list, Kavita's API key expiry.
    pub fn test_request(&self) -> ApiRequest {
        match &self.kind {
            Kind::Komga { .. } => self.komga_libraries(),
            Kind::Kavita => self.request(HttpMethod::Get, "/api/plugin/authkey-expires"),
        }
    }

    /// "Komga" or "Kavita".
    pub fn name(&self) -> &'static str {
        match self.kind {
            Kind::Komga { .. } => "Komga",
            Kind::Kavita => "Kavita",
        }
    }

    fn komga_libraries(&self) -> ApiRequest {
        self.request(HttpMethod::Get, "/api/v1/libraries")
    }

    fn request(&self, method: HttpMethod, path: &str) -> ApiRequest {
        let header = match self.kind {
            Kind::Komga { .. } => "X-API-Key",
            Kind::Kavita => "x-api-key",
        };
        ApiRequest {
            method,
            url: format!("{}{path}", self.url),
            headers: vec![(header.into(), self.api_key.clone())],
        }
    }
}

/// A library a Komga API key can see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(as = ComicServerLibrary)]
pub struct Library {
    pub id: String,
    pub name: String,
}

/// What "Test connection" found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    /// The libraries the key can see.
    Komga { libraries: Vec<Library> },
    /// When the key expires (as Kavita writes it); `None` when it never does.
    Kavita { key_expires: Option<String> },
}

/// Sends [`ComicServer`] requests. Cheap to clone.
#[derive(Debug, Clone)]
pub struct ComicServerClient {
    http: reqwest::Client,
}

impl ComicServerClient {
    pub fn new() -> Result<Self, ComicServerError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| ComicServerError::Request(describe(&e)))?;
        Ok(Self { http })
    }

    /// Asks `server` to scan: Komga's library, or each library the key can see; every Kavita
    /// library. A library that fails does not stop the others; the first failure is returned.
    pub async fn scan(&self, server: &ComicServer) -> Result<(), ComicServerError> {
        let libraries = match server.libraries_request() {
            Some(request) => self.libraries(request).await?,
            None => Vec::new(),
        };
        let ids: Vec<String> = libraries.into_iter().map(|l| l.id).collect();
        let mut first_error = None;
        for request in server.scan_requests(&ids) {
            if let Err(e) = self.send(request).await {
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Checks the URL and API key with a request that changes nothing.
    pub async fn test(&self, server: &ComicServer) -> Result<Connection, ComicServerError> {
        let request = server.test_request();
        match server.kind {
            Kind::Komga { .. } => Ok(Connection::Komga {
                libraries: self.libraries(request).await?,
            }),
            Kind::Kavita => {
                let expires: serde_json::Value = json(self.send(request).await?).await?;
                let key_expires = match expires {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(date) => Some(date),
                    other => Some(other.to_string()),
                };
                Ok(Connection::Kavita { key_expires })
            }
        }
    }

    async fn libraries(&self, request: ApiRequest) -> Result<Vec<Library>, ComicServerError> {
        json(self.send(request).await?).await
    }

    async fn send(&self, request: ApiRequest) -> Result<reqwest::Response, ComicServerError> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
        };
        let mut builder = self.http.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .send()
            .await
            .map_err(|e| ComicServerError::Request(describe(&e)))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ComicServerError::Status(status.as_u16()));
        }
        Ok(response)
    }
}

async fn json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ComicServerError> {
    let body = response
        .bytes()
        .await
        .map_err(|e| ComicServerError::Request(describe(&e)))?;
    serde_json::from_slice(&body).map_err(|e| ComicServerError::Body(e.to_string()))
}

/// The error with its causes, e.g. "error sending request: connection refused".
fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text = format!("{text}: {cause}");
        source = cause.source();
    }
    text
}

/// Runs an action once, `window` after the last of a burst of triggers. Stops when dropped.
pub struct Debounce {
    tx: mpsc::UnboundedSender<()>,
}

impl Debounce {
    /// Spawns the waiting task on the current tokio runtime. A run in progress delays the next
    /// one until it ends.
    pub fn spawn<F, Fut>(window: Duration, mut action: F) -> Self
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();
        tokio::spawn(async move {
            // Waits for a first trigger, then until `window` passes without another.
            while rx.recv().await.is_some() {
                loop {
                    match tokio::time::timeout(window, rx.recv()).await {
                        Ok(Some(())) => continue,
                        Ok(None) => return,
                        Err(_) => break,
                    }
                }
                action().await;
            }
        });
        Self { tx }
    }

    pub fn trigger(&self) {
        // Fails only once the task ended, which only dropping `self` does.
        let _ = self.tx.send(());
    }
}
