//! Asking a comic server (Komga, Kavita) to scan its libraries, so new downloads show up before
//! its next scheduled scan (issue #269). No FMD2 counterpart.

use std::future::Future;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::settings::{ComicServerKind, ComicServerSettings};

/// How long after the last download finished the comic server is asked to scan, so a batch of
/// downloads gives one scan.
pub const SCAN_DELAY: Duration = Duration::from_secs(60);

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
