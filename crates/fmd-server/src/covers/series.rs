//! `GET /api/covers/series`: a list title's cover. A list has no cover column (FMD2's per-site
//! tables don't either, baseunits/DBDataProcess.pas:143-153), so the cover link is looked up once
//! per title and stored: from the title's MangaBaka match when there is one, else from the
//! website's `GetInfo` (`MangaInfo.CoverLink`, baseunits/lua/LuaMangaInfo.pas:27).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use fmd_core::settings::{ModuleOverrides, effective_limits};
use fmd_store::{CoverLink, CoverSource, ListsDb};
use serde::Deserialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;
use utoipa::IntoParams;

use super::{Covers, Origin, check_width, respond};
use crate::error::ApiQuery;
use crate::series::fetch_info;
use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

/// `GetInfo` lookups for covers that run at once per module, at most: covers are not worth
/// keeping a website busy for.
const MAX_LOOKUPS: usize = 3;

/// A MangaBaka thumbnail wider than this is taken from the 350 px tall one instead of the 250 px
/// tall one (about 178 px wide at a 5:7 ratio).
const LARGE_FROM_WIDTH: u32 = 185;

/// The per-module queues of cover lookups.
#[derive(Default)]
pub(crate) struct Lookups {
    /// Each module's permits, with how many it was made with.
    modules: Mutex<HashMap<String, (usize, Arc<Semaphore>)>>,
}

impl Lookups {
    /// Waits for a turn to look up a cover of `module`, which may run `permits` lookups at once.
    /// Waiting is first come, first served, and dropping the future leaves the queue.
    async fn turn(&self, module: &str, permits: usize) -> Result<OwnedSemaphorePermit, ApiError> {
        let semaphore = {
            let mut modules = self.modules.lock().unwrap_or_else(|e| e.into_inner());
            let entry = modules
                .entry(module.to_owned())
                .or_insert_with(|| (permits, Arc::new(Semaphore::new(permits))));
            // The module's limits changed: lookups already queued finish on the old permits.
            if entry.0 != permits {
                *entry = (permits, Arc::new(Semaphore::new(permits)));
            }
            entry.1.clone()
        };
        semaphore
            .acquire_owned()
            .await
            .map_err(|e| ApiError::Internal(format!("cover lookups: {e}")))
    }
}

/// The `GET /api/covers/series` URL of `module`'s title at `link`.
pub(crate) fn cover_url(module: &str, link: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("module", module)
        .append_pair("link", link)
        .finish();
    format!("/api/covers/series?{query}")
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SeriesCoverQuery {
    /// The website module the title is listed by.
    module: String,
    /// The title's link relative to the module's `RootURL`, as its list has it.
    link: String,
    /// Scale down to this width in pixels (1 to 2000), keeping the aspect ratio.
    w: Option<u32>,
}

/// A list title's cover. Its link comes from, in order: the link stored for the title while it
/// is recent (`covers.revalidate_after_hours`); the title's accepted match in the MangaBaka
/// database, when it is downloaded; the website module's `GetInfo`, a few at a time per module.
/// The link found is stored, "no cover" included. The image then goes through the cover cache
/// like `GET /api/covers`. With `general.load_covers` off nothing is looked up or fetched.
#[utoipa::path(get, path = "/api/covers/series", tag = "covers", operation_id = "getSeriesCover",
    params(SeriesCoverQuery),
    responses(
        (status = 200, description = "The cover image", content_type = "image/*"),
        (status = 304, description = "The browser's copy (`If-None-Match`) is current"),
        (status = 400, description = "A bad width, or a cover link that may not be fetched", body = Problem),
        (status = 404, description = "Unknown module, the title has no cover, or covers are off", body = Problem),
        (status = 502, description = "The website or the cover's host failed", body = Problem),
        (status = 503, description = "lists.db is not open", body = Problem),
    ))]
pub(crate) async fn get(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<SeriesCoverQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let covers = state.covers.clone().ok_or(ApiError::NotFound)?;
    check_width(query.w)?;
    if !state.settings.get().general.load_covers {
        return Err(ApiError::Missing("loading covers is off".into()));
    }
    let lists = state.lists()?;
    let cover = {
        // One lookup per title at a time; the others find its link stored.
        let _inflight = covers
            .inflight(&format!("series\0{}\0{}", query.module, query.link))
            .await;
        find(&state, &covers, &lists, &query.module, &query.link).await?
    };
    let Some((origin, url)) = cover.and_then(|c| pick(&query.module, c, query.w)) else {
        return Err(ApiError::Missing("the title has no cover".into()));
    };
    let entry = covers.get(&origin, &url, query.w).await?;
    Ok(respond(entry, &headers, covers.revalidate_after))
}

/// The cover to fetch for a `w` wide thumbnail of `cover`, and with whose session.
fn pick(module: &str, cover: CoverLink, w: Option<u32>) -> Option<(Origin, Url)> {
    let url = match cover.source {
        CoverSource::MangaBaka if w.is_none_or(|w| w > LARGE_FROM_WIDTH) => {
            cover.large_url.or(cover.url)
        }
        _ => cover.url,
    }?;
    let origin = match cover.source {
        CoverSource::MangaBaka => Origin::Plain,
        CoverSource::Website => Origin::Module(module.to_owned()),
    };
    Url::parse(&url).ok().map(|url| (origin, url))
}

/// The cover link of `module`'s title at `link`: stored, else looked up and stored.
async fn find(
    state: &AppState,
    covers: &Covers,
    lists: &ListsDb,
    module: &str,
    link: &str,
) -> Result<Option<CoverLink>, ApiError> {
    let Some(info) = state.modules.module(module) else {
        return Err(ApiError::Missing(format!("no module {module:?}")));
    };
    let now = now_secs();
    let stored = {
        let (lists, module, link) = (lists.clone(), module.to_owned(), link.to_owned());
        off_thread(move || lists.cover_links().get(&module, &link)).await??
    };
    let max_age = i64::try_from(covers.revalidate_after.as_secs()).unwrap_or(i64::MAX);
    if let Some(stored) = stored
        && now.saturating_sub(stored.checked_at) < max_age
    {
        return Ok(Some(stored));
    }
    let found = match mangabaka(state, lists, module, link, now).await? {
        Some(found) => found,
        None => {
            let _turn = covers
                .lookups
                .turn(module, permits(state, &info.limits, module).await)
                .await?;
            website(state, &info.root_url, module, link, now).await?
        }
    };
    let (lists, module, link, stored) = (
        lists.clone(),
        module.to_owned(),
        link.to_owned(),
        found.clone(),
    );
    off_thread(move || lists.cover_links().put(&module, &link, &stored)).await??;
    Ok(Some(found))
}

/// How many `GetInfo` lookups for covers `module` may run at once: [`MAX_LOOKUPS`], or fewer
/// when its connection limit (the user's override included) is lower.
async fn permits(
    state: &AppState,
    limits: &fmd_core::settings::ModuleLimits,
    module: &str,
) -> usize {
    let id = module.to_owned();
    let overrides = state
        .blocking(move |db| ModuleOverrides::load(&db.module_settings(), &id))
        .await
        .map_err(|e| tracing::warn!(target: "fmd_server", "settings of module {module}: {e}"))
        .ok();
    let connections = effective_limits(
        limits,
        overrides.as_ref(),
        &state.settings.get().connections,
    )
    .max_connections;
    match usize::try_from(connections) {
        Ok(0) | Err(_) => MAX_LOOKUPS,
        Ok(n) => n.min(MAX_LOOKUPS),
    }
}

/// The cover of `module`'s title at `link` in the MangaBaka database: its accepted match's
/// thumbnails. `None` without the database, an accepted match, or a thumbnail.
async fn mangabaka(
    state: &AppState,
    lists: &ListsDb,
    module: &str,
    link: &str,
    now: i64,
) -> Result<Option<CoverLink>, ApiError> {
    let Some(meta) = state.metadata.as_ref().and_then(|m| m.current()) else {
        return Ok(None);
    };
    let (lists, module, link) = (lists.clone(), module.to_owned(), link.to_owned());
    let found = off_thread(move || -> Result<_, ApiError> {
        let Some(m) = lists.matches().get(&module, &link)? else {
            return Ok(None);
        };
        let Some(id) = m.series_id.filter(|_| m.confidence.is_accepted()) else {
            return Ok(None);
        };
        let Some(series) = meta.series(id)? else {
            return Ok(None);
        };
        if series.cover_x250.is_none() && series.cover_x350.is_none() {
            return Ok(None);
        }
        Ok(Some(CoverLink {
            url: series.cover_x250,
            large_url: series.cover_x350,
            source: CoverSource::MangaBaka,
            series_id: Some(id),
            checked_at: now,
        }))
    })
    .await??;
    Ok(found)
}

/// The cover of `module`'s title at `link` from the module's `GetInfo`. A title the module finds
/// nothing at has no cover.
async fn website(
    state: &AppState,
    root_url: &str,
    module: &str,
    link: &str,
    now: i64,
) -> Result<CoverLink, ApiError> {
    let url = match fetch_info(state, module, link).await {
        Ok(info) => absolute(root_url, &info.cover_link),
        Err(ApiError::Missing(_)) => None,
        Err(e) => return Err(e),
    };
    Ok(website_link(url, now))
}

/// Stores the cover `GetInfo` gave the series page for `module`'s title at `link`, saving a
/// lookup later, unless the title's cover comes from MangaBaka. A failure only logs.
pub(crate) async fn learned(state: &AppState, module: &str, link: &str, cover: &str) {
    let (Some(lists), Some(info)) = (state.lists.clone(), state.modules.module(module)) else {
        return;
    };
    let found = website_link(absolute(&info.root_url, cover), now_secs());
    let (module, link) = (module.to_owned(), link.to_owned());
    let stored = off_thread(move || -> Result<(), fmd_store::StoreError> {
        let covers = lists.cover_links();
        if covers
            .get(&module, &link)?
            .is_some_and(|c| c.source == CoverSource::MangaBaka)
        {
            return Ok(());
        }
        covers.put(&module, &link, &found)
    })
    .await;
    match stored {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(target: "fmd_server", "storing a cover link: {e}"),
        Err(e) => tracing::warn!(target: "fmd_server", "storing a cover link: {e}"),
    }
}

/// A cover link from the website.
fn website_link(url: Option<String>, now: i64) -> CoverLink {
    CoverLink {
        url,
        large_url: None,
        source: CoverSource::Website,
        series_id: None,
        checked_at: now,
    }
}

/// `cover` (a `MangaInfo.CoverLink`) as an absolute URL, resolved against the module's root;
/// `None` when it is empty or not a URL.
fn absolute(root_url: &str, cover: &str) -> Option<String> {
    let cover = cover.trim();
    if cover.is_empty() {
        return None;
    }
    let url = match Url::parse(cover) {
        Ok(url) => url,
        Err(_) => Url::parse(root_url).ok()?.join(cover).ok()?,
    };
    Some(url.into())
}

/// Now, in seconds since the Unix epoch.
fn now_secs() -> i64 {
    i64::try_from(super::cache::now()).unwrap_or(i64::MAX)
}
