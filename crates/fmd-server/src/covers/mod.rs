//! `GET /api/covers`: manga covers fetched with the owning module's HTTP settings (sites often
//! block hotlinking) and cached on disk.

mod cache;
mod fetch;
mod thumbnail;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use fmd_core::settings::CoverSettings;
use fmd_http::HttpSession;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use url::Url;
use utoipa::IntoParams;

use self::cache::{DiskCache, Entry};
use self::fetch::{FetchError, Fetched, Validators};
use crate::error::ApiQuery;
use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

/// How the cover cache behaves.
#[derive(Debug, Clone)]
pub struct CoverConfig {
    /// Where cached covers live, e.g. `<data dir>/covers`.
    pub dir: PathBuf,
    /// How long a cached cover is served before upstream is asked whether it changed.
    pub revalidate_after: Duration,
    /// Size cap of the cache directory; least recently used covers are evicted past it.
    pub max_bytes: u64,
}

impl CoverConfig {
    /// Caching in `dir` with the default [`CoverSettings`].
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self::from_settings(dir, &CoverSettings::default())
    }

    /// Caching in `dir` as `settings` say.
    pub fn from_settings(dir: impl AsRef<Path>, settings: &CoverSettings) -> Self {
        Self {
            dir: dir.as_ref().to_owned(),
            revalidate_after: Duration::from_secs(
                u64::from(settings.revalidate_after_hours) * 3600,
            ),
            max_bytes: u64::from(settings.cache_size_mb) * 1024 * 1024,
        }
    }
}

/// A session to fetch one cover with, and the root URL of the module it belongs to.
pub struct CoverSession {
    /// The module's `RootURL`; covers are fetched with `Referer: <root_url>/`.
    pub root_url: String,
    /// A session set up like FMD2's `TModuleContainer.CreateHTTP`
    /// (baseunits/WebsiteModules.pas:353-387): the module's cookie jar, connection queue, user
    /// agent and proxy (`fmd_lua::create_http`).
    pub session: HttpSession,
}

/// The website modules covers are fetched for.
pub trait CoverModules: Send + Sync + 'static {
    /// A session for module `id`, or `None` when there is no such module. Called on a thread
    /// outside the tokio runtime, so it may block.
    fn cover_session(&self, id: &str) -> Option<CoverSession>;
}

/// The proxy URL serving `url`, a cover of module `module`; what other endpoints hand the browser
/// instead of the site's own cover link.
pub fn cover_url(module: &str, url: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("module", module)
        .append_pair("url", url)
        .finish();
    format!("/api/covers?{query}")
}

/// The cover service behind the handler.
pub(crate) struct Covers {
    cache: Arc<DiskCache>,
    modules: Arc<dyn CoverModules>,
    revalidate_after: Duration,
    /// One lock per cover being resolved, so concurrent requests make one upstream fetch.
    inflight: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Covers {
    pub(crate) fn new(config: CoverConfig, modules: Arc<dyn CoverModules>) -> Self {
        Self {
            cache: Arc::new(DiskCache::new(config.dir, config.max_bytes)),
            modules,
            revalidate_after: config.revalidate_after,
            inflight: Mutex::default(),
        }
    }

    /// The cover at `url` of `module`, from the cache while fresh, else from upstream. Concurrent
    /// calls for one cover wait for the first, then find its result in the cache.
    async fn get(&self, module: &str, url: &Url, width: Option<u32>) -> Result<Entry, ApiError> {
        let key = cache_key(module, url);
        let _inflight = self.inflight(&key).await;
        let cover = self.resolve(key.clone(), module, url).await?;
        match width {
            Some(width) => self.thumbnail(key, cover, width).await,
            None => Ok(cover),
        }
    }

    /// `cover` scaled to `width`, cached separately and remade when the cover changes.
    async fn thumbnail(&self, key: String, cover: Entry, width: u32) -> Result<Entry, ApiError> {
        let cache = self.cache.clone();
        off_thread(move || {
            let key = format!("{key}-w{width}");
            if let Some(thumb) = cache.load(&key)
                && thumb.meta.source_etag.as_ref() == Some(&cover.meta.etag)
            {
                return Ok(thumb);
            }
            let made = thumbnail::thumbnail(&cover.body, width)
                .map_err(|e| ApiError::Internal(format!("making a thumbnail: {e}")))?;
            let Some((body, content_type)) = made else {
                return Ok(cover);
            };
            let thumb = Entry::new(
                body,
                content_type.to_owned(),
                Validators::default(),
                cover.meta.fetched_at,
                Some(cover.meta.etag),
            );
            cache.store(&key, &thumb).map_err(cache_error)?;
            Ok(thumb)
        })
        .await?
    }

    /// Waits for the lock of cover `key`, created on first use.
    async fn inflight(&self, key: &str) -> Inflight<'_> {
        let lock = {
            let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            map.entry(key.to_owned()).or_default().clone()
        };
        // Built before waiting, so a request cancelled while it waits still cleans up.
        let mut inflight = Inflight {
            map: &self.inflight,
            key: key.to_owned(),
            lock: Some(lock.clone()),
            guard: None,
        };
        inflight.guard = Some(lock.lock_owned().await);
        inflight
    }

    async fn resolve(&self, key: String, module: &str, url: &Url) -> Result<Entry, ApiError> {
        let cache = self.cache.clone();
        let cached = {
            let key = key.clone();
            off_thread(move || cache.load(&key)).await?
        };
        if let Some(entry) = &cached
            && cache::now().saturating_sub(entry.meta.fetched_at) < self.revalidate_after.as_secs()
        {
            return Ok(entry.clone());
        }
        let validators = cached.as_ref().map(|e| e.meta.upstream.clone());
        let modules = self.modules.clone();
        let (module, target) = (module.to_owned(), url.clone());
        let fetched = on_fetch_thread(move || {
            fetch::fetch(modules.as_ref(), &module, &target, validators.as_ref())
        })
        .await?;
        let cache = self.cache.clone();
        match (fetched, cached) {
            (Ok(Fetched::NotModified), Some(mut entry)) => {
                entry.meta.fetched_at = cache::now();
                let meta = entry.meta.clone();
                off_thread(move || cache.store_meta(&key, &meta))
                    .await?
                    .map_err(cache_error)?;
                Ok(entry)
            }
            (
                Ok(Fetched::Body {
                    body,
                    content_type,
                    validators,
                }),
                _,
            ) => {
                let entry = Entry::new(body, content_type, validators, cache::now(), None);
                let stored = entry.clone();
                off_thread(move || cache.store(&key, &stored))
                    .await?
                    .map_err(cache_error)?;
                Ok(entry)
            }
            (Ok(Fetched::NotModified), None) => Err(ApiError::BadGateway(
                "upstream answered 304 to a plain GET".into(),
            )),
            // A site that is down keeps its covers: serve the stale copy.
            (Err(FetchError::Upstream(msg)), Some(entry)) => {
                tracing::debug!(target: "fmd_server", "serving a stale cover: {msg}");
                Ok(entry)
            }
            (Err(err), _) => Err(err.into()),
        }
    }
}

impl From<FetchError> for ApiError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::UnknownModule(_) => ApiError::NotFound,
            FetchError::Forbidden(msg) => ApiError::BadRequest(msg),
            FetchError::Upstream(msg) => ApiError::BadGateway(msg),
            FetchError::Http(e) => ApiError::Internal(e.to_string()),
        }
    }
}

fn cache_error(err: std::io::Error) -> ApiError {
    ApiError::Internal(format!("cover cache: {err}"))
}

/// A held per-cover lock. Dropping it (also when the request is cancelled) releases the lock and
/// removes it from the map once nobody else holds or waits for it.
struct Inflight<'a> {
    map: &'a Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    key: String,
    lock: Option<Arc<tokio::sync::Mutex<()>>>,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        self.guard.take();
        self.lock.take();
        // Every request takes its first reference under the map's mutex and keeps one until it
        // gets here, so a count of one (the map's) means no request holds or waits for the lock.
        if map
            .get(&self.key)
            .is_some_and(|l| Arc::strong_count(l) == 1)
        {
            map.remove(&self.key);
        }
    }
}

/// Runs `f` on a new OS thread: `fmd_http` sessions block, and refuse to on the runtime's threads
/// (the blocking pool included).
async fn on_fetch_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    let (tx, rx) = oneshot::channel();
    std::thread::Builder::new()
        .name("fmd-cover".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| ApiError::Internal(format!("spawning a cover fetch: {e}")))?;
    rx.await
        .map_err(|_| ApiError::Internal("cover fetch thread died".into()))
}

/// The cache key of a cover: a hash of the module and URL.
fn cache_key(module: &str, url: &Url) -> String {
    hex(&Sha256::new()
        .chain_update(module.as_bytes())
        .chain_update([0])
        .chain_update(url.as_str().as_bytes())
        .finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CoverQuery {
    /// The website module the cover belongs to.
    module: String,
    /// The cover's URL on the site (http or https).
    url: String,
    /// Scale down to this width in pixels (1 to 2000), keeping the aspect ratio.
    w: Option<u32>,
}

/// Widest thumbnail `?w=` asks for.
const MAX_WIDTH: u32 = 2000;

/// A manga cover, fetched with its module's cookies, user agent and referer and cached on disk.
#[utoipa::path(get, path = "/api/covers", tag = "covers", operation_id = "getCover",
    params(CoverQuery),
    responses(
        (status = 200, description = "The cover image", content_type = "image/*"),
        (status = 304, description = "The browser's copy (`If-None-Match`) is current"),
        (status = 400, description = "Not an http(s) URL, or a private-network target", body = Problem),
        (status = 404, description = "Unknown module", body = Problem),
        (status = 502, description = "Upstream failed to deliver the cover", body = Problem),
    ))]
pub(crate) async fn get(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<CoverQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let covers = state.covers.clone().ok_or(ApiError::NotFound)?;
    // The scheme is checked with the rest of the SSRF guard, before anything is fetched.
    let url = Url::parse(&query.url)
        .map_err(|e| ApiError::BadRequest(format!("bad URL {:?}: {e}", query.url)))?;
    if query.w.is_some_and(|w| w == 0 || w > MAX_WIDTH) {
        return Err(ApiError::BadRequest(format!(
            "w must be between 1 and {MAX_WIDTH}"
        )));
    }
    let entry = covers.get(&query.module, &url, query.w).await?;
    Ok(respond(entry, &headers, covers.revalidate_after))
}

/// 304 when the browser already has `entry` (RFC 9110 `If-None-Match`), else the image; either
/// way with its ETag and how long the browser may keep it.
fn respond(entry: Entry, request: &HeaderMap, max_age: Duration) -> Response {
    let validators = [
        (header::ETAG, entry.meta.etag.clone()),
        (
            header::CACHE_CONTROL,
            format!("private, max-age={}", max_age.as_secs()),
        ),
        // Only images are cached, but never let a browser sniff one into something else.
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
    ];
    if browser_has(request, &entry.meta.etag) {
        return (StatusCode::NOT_MODIFIED, validators).into_response();
    }
    (
        validators,
        [(header::CONTENT_TYPE, entry.meta.content_type)],
        entry.body,
    )
        .into_response()
}

/// Whether the request's `If-None-Match` lists `etag` (weakly compared) or is `*`.
fn browser_has(request: &HeaderMap, etag: &str) -> bool {
    let etag = etag.trim_start_matches("W/");
    request
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.trim_start_matches("W/") == etag)
}
