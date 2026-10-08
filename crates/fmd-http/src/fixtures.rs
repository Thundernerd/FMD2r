//! HTTP fixtures: a [`Transport`] that records every exchange to a directory, and one that
//! serves them back offline. The format is documented in `docs/fixtures.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::decode::decode;
use crate::transport::{BoxFuture, Transport, TransportError, WireRequest, WireResponse};

/// The version of the fixture format `index.json` declares.
pub const FIXTURE_FORMAT: u32 = 1;

const INDEX_FILE: &str = "index.json";
const EXCHANGES_DIR: &str = "exchanges";
const BODIES_DIR: &str = "bodies";

/// Errors reading or writing a fixture directory.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path}: fixture format {found}, expected {FIXTURE_FORMAT}")]
    Format { path: PathBuf, found: u32 },
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> FixtureError + '_ {
    move |source| FixtureError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `index.json`: every exchange, in the order they were sent.
#[derive(Debug, Serialize, Deserialize)]
struct Index {
    format: u32,
    exchanges: Vec<IndexEntry>,
}

/// One exchange in the index, enough to find it by eye.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexEntry {
    /// The exchange's file in `exchanges/`, without its `.json` extension.
    id: String,
    method: String,
    url: String,
    /// The response status; `None` for a transport error.
    status: Option<u16>,
}

/// `exchanges/<id>.json`.
#[derive(Debug, Serialize, Deserialize)]
struct Exchange {
    request: RecordedRequest,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Debug, Serialize, Deserialize)]
struct RecordedRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    /// The body's file, relative to the fixture directory; `None` for no body.
    body: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Response(RecordedResponse),
    /// The transport failed with this message.
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct RecordedResponse {
    status: u16,
    reason: String,
    /// The headers as received, `Content-Encoding` included.
    headers: Vec<(String, String)>,
    /// The body's file, relative to the fixture directory; `None` for no body.
    body: Option<String>,
    /// The `Content-Encoding` the body was decoded from before it was stored; `None` when it
    /// is stored as received.
    decoded_from: Option<String>,
}

/// The first value of header `name`, ignoring ASCII case.
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sends through another transport and writes every exchange to a fixture directory.
///
/// Response bodies are stored decoded, as `fmd-http` decodes them after the exchange
/// (baseunits/httpsendthread.pas:681-710), so fixtures stay readable and diffable.
pub struct RecordingTransport {
    inner: Arc<dyn Transport>,
    state: Arc<Recorder>,
}

struct Recorder {
    dir: PathBuf,
    next: AtomicUsize,
    index: Mutex<Vec<IndexEntry>>,
}

impl RecordingTransport {
    /// Records the exchanges `inner` sends into `dir`, replacing the `index.json`,
    /// `exchanges/` and `bodies/` a previous recording left there.
    pub fn new(dir: impl Into<PathBuf>, inner: Arc<dyn Transport>) -> Result<Self, FixtureError> {
        let dir = dir.into();
        for sub in [EXCHANGES_DIR, BODIES_DIR] {
            let path = dir.join(sub);
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(io_error(&path))?;
            }
            std::fs::create_dir_all(&path).map_err(io_error(&path))?;
        }
        let state = Recorder {
            dir,
            next: AtomicUsize::new(1),
            index: Mutex::default(),
        };
        state.write_index(&[])?;
        Ok(Self {
            inner,
            state: Arc::new(state),
        })
    }
}

impl Recorder {
    fn write_index(&self, exchanges: &[IndexEntry]) -> Result<(), FixtureError> {
        let index = Index {
            format: FIXTURE_FORMAT,
            exchanges: exchanges.to_vec(),
        };
        write_json(&self.dir.join(INDEX_FILE), &index)
    }

    /// Writes `body` as `bodies/<id>.<kind>`; `None` for an empty body.
    fn write_body(
        &self,
        id: &str,
        kind: &str,
        body: &[u8],
    ) -> Result<Option<String>, FixtureError> {
        if body.is_empty() {
            return Ok(None);
        }
        let name = format!("{BODIES_DIR}/{id}.{kind}");
        let path = self.dir.join(&name);
        std::fs::write(&path, body).map_err(io_error(&path))?;
        Ok(Some(name))
    }

    fn record(
        &self,
        id: &str,
        request: &WireRequest,
        response: &Result<WireResponse, TransportError>,
    ) -> Result<(), FixtureError> {
        let outcome = match response {
            Ok(response) => {
                let encoding = header(&response.headers, "Content-Encoding")
                    .filter(|_| !response.body.is_empty())
                    .map(str::to_owned);
                let body = match &encoding {
                    Some(encoding) => decode(encoding, response.body.clone()),
                    None => response.body.clone(),
                };
                Outcome::Response(RecordedResponse {
                    status: response.status,
                    reason: response.reason.clone(),
                    headers: response.headers.clone(),
                    body: self.write_body(id, "response", &body)?,
                    decoded_from: encoding,
                })
            }
            Err(e) => Outcome::Error(e.0.clone()),
        };
        let status = match &outcome {
            Outcome::Response(r) => Some(r.status),
            Outcome::Error(_) => None,
        };
        let exchange = Exchange {
            request: RecordedRequest {
                method: request.method.clone(),
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: self.write_body(id, "request", &request.body)?,
            },
            outcome,
        };
        write_json(
            &self.dir.join(EXCHANGES_DIR).join(format!("{id}.json")),
            &exchange,
        )?;
        let mut index = lock(&self.index);
        index.push(IndexEntry {
            id: id.to_owned(),
            method: request.method.clone(),
            url: request.url.clone(),
            status,
        });
        index.sort_by(|a, b| a.id.cmp(&b.id));
        self.write_index(&index)
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), FixtureError> {
    let mut json = serde_json::to_vec_pretty(value).map_err(json_error(path))?;
    json.push(b'\n');
    std::fs::write(path, json).map_err(io_error(path))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, FixtureError> {
    let bytes = std::fs::read(path).map_err(io_error(path))?;
    serde_json::from_slice(&bytes).map_err(json_error(path))
}

fn json_error(path: &Path) -> impl FnOnce(serde_json::Error) -> FixtureError + '_ {
    move |source| FixtureError::Json {
        path: path.to_path_buf(),
        source,
    }
}

impl Transport for RecordingTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        // Numbered when sent, so the index lists exchanges in the order they started.
        let id = format!("{:04}", self.state.next.fetch_add(1, Ordering::SeqCst));
        let sent = self.inner.send(request.clone());
        let state = self.state.clone();
        Box::pin(async move {
            let response = sent.await;
            state
                .record(&id, &request, &response)
                .map_err(|e| TransportError(format!("recording the exchange: {e}")))?;
            response
        })
    }
}

/// How a [`ReplayTransport`] matches a request to a recorded exchange.
#[derive(Debug, Clone, Default)]
pub struct ReplayOptions {
    /// Request headers whose values must match too, compared by name ignoring ASCII case and
    /// by value with surrounding blanks trimmed. Method, URL and body always match exactly;
    /// every other header is ignored.
    pub match_headers: Vec<String>,
}

/// Serves the exchanges of a fixture directory instead of the network.
///
/// A request is answered by the recorded exchanges with its method, URL, body and
/// [`match_headers`](ReplayOptions::match_headers), in recording order; once they are used up
/// the last one answers again. A request with no recorded exchange fails like a transport error
/// and is listed in [`misses`](Self::misses).
pub struct ReplayTransport {
    options: ReplayOptions,
    exchanges: HashMap<Key, Vec<Replayed>>,
    served: Mutex<HashMap<Key, usize>>,
    misses: Mutex<Vec<String>>,
}

/// What a request is matched by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    method: String,
    url: String,
    body: Vec<u8>,
    headers: Vec<Option<String>>,
}

impl Key {
    fn new(
        options: &ReplayOptions,
        method: &str,
        url: &str,
        body: Vec<u8>,
        headers: &[(String, String)],
    ) -> Key {
        Key {
            method: method.to_owned(),
            url: url.to_owned(),
            body,
            headers: header_values(options, headers),
        }
    }
}

#[derive(Debug, Clone)]
enum Replayed {
    Response(WireResponse),
    Error(String),
}

impl ReplayTransport {
    /// Loads the fixture directory `dir`.
    pub fn open(dir: impl AsRef<Path>, options: ReplayOptions) -> Result<Self, FixtureError> {
        let dir = dir.as_ref();
        let index_path = dir.join(INDEX_FILE);
        let index: Index = read_json(&index_path)?;
        if index.format != FIXTURE_FORMAT {
            return Err(FixtureError::Format {
                path: index_path,
                found: index.format,
            });
        }
        let read_body = |name: &Option<String>| -> Result<Vec<u8>, FixtureError> {
            match name {
                Some(name) => {
                    let path = dir.join(name);
                    std::fs::read(&path).map_err(io_error(&path))
                }
                None => Ok(Vec::new()),
            }
        };
        let mut exchanges: HashMap<Key, Vec<Replayed>> = HashMap::new();
        for entry in &index.exchanges {
            let path = dir.join(EXCHANGES_DIR).join(format!("{}.json", entry.id));
            let exchange: Exchange = read_json(&path)?;
            let request = &exchange.request;
            let key = Key::new(
                &options,
                &request.method,
                &request.url,
                read_body(&request.body)?,
                &request.headers,
            );
            let replayed = match exchange.outcome {
                Outcome::Response(response) => {
                    let mut headers = response.headers;
                    // The body is stored decoded: serve it without the encoding header, so the
                    // session does not decode it again.
                    if response.decoded_from.is_some() {
                        headers.retain(|(n, _)| !n.eq_ignore_ascii_case("Content-Encoding"));
                    }
                    Replayed::Response(WireResponse {
                        status: response.status,
                        reason: response.reason,
                        headers,
                        body: read_body(&response.body)?,
                    })
                }
                Outcome::Error(message) => Replayed::Error(message),
            };
            exchanges.entry(key).or_default().push(replayed);
        }
        Ok(Self {
            options,
            exchanges,
            served: Mutex::default(),
            misses: Mutex::default(),
        })
    }

    /// Every request that had no recorded exchange, as `METHOD URL`, in the order they came.
    pub fn misses(&self) -> Vec<String> {
        lock(&self.misses).clone()
    }

    fn answer(&self, request: &WireRequest) -> Result<WireResponse, TransportError> {
        let key = Key::new(
            &self.options,
            &request.method,
            &request.url,
            request.body.clone(),
            &request.headers,
        );
        let Some(recorded) = self.exchanges.get(&key) else {
            let miss = format!("{} {}", request.method, request.url);
            lock(&self.misses).push(miss.clone());
            return Err(TransportError(format!("no recorded exchange for {miss}")));
        };
        let i = {
            let mut served = lock(&self.served);
            let count = served.entry(key).or_default();
            let i = (*count).min(recorded.len().saturating_sub(1));
            *count += 1;
            i
        };
        match recorded.get(i) {
            Some(Replayed::Response(response)) => Ok(response.clone()),
            Some(Replayed::Error(message)) => Err(TransportError(message.clone())),
            None => Err(TransportError("empty recording".into())),
        }
    }
}

/// The values of the headers `options` matches on, in its order.
fn header_values(options: &ReplayOptions, headers: &[(String, String)]) -> Vec<Option<String>> {
    options
        .match_headers
        .iter()
        .map(|name| header(headers, name).map(|v| v.trim().to_owned()))
        .collect()
}

impl Transport for ReplayTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let answer = self.answer(&request);
        Box::pin(async move { answer })
    }
}
