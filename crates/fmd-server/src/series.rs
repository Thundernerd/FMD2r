//! `POST /api/resolve` (which module handles a pasted URL) and `GET /api/series` (a series'
//! info and chapters, as FMD2's info panel shows them).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use fmd_core::info::{InfoError, InfoOptions, MangaInfo};
use fmd_core::modules::locate_by_url;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiJson, ApiQuery};
use crate::{ApiError, AppState, Problem, cover_url};

/// A manga URL to resolve.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ResolveRequest {
    pub url: String,
}

/// A series: its module and its link relative to the module's `RootURL`.
#[derive(Debug, Serialize, ToSchema)]
pub struct SeriesRef {
    pub module_id: String,
    pub link: String,
}

#[utoipa::path(post, path = "/api/resolve", tag = "series", operation_id = "resolveUrl",
    request_body = ResolveRequest,
    responses(
        (status = 200, body = SeriesRef, description = "The module and the module-relative link of the series"),
        (status = 404, body = Problem, description = "No module handles this URL"),
    ))]
pub(crate) async fn resolve(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<ResolveRequest>,
) -> Result<Json<SeriesRef>, ApiError> {
    let modules = state.modules.modules();
    let located = locate_by_url(&modules, &req.url)
        .ok_or_else(|| ApiError::Missing(format!("no module handles {}", req.url)))?;
    Ok(Json(SeriesRef {
        module_id: located.module.id.clone(),
        link: located.link,
    }))
}

/// Which series to show.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SeriesQuery {
    /// Module ID.
    module: String,
    /// The series link relative to the module's `RootURL`.
    link: String,
}

/// A series as FMD2's info panel shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct SeriesInfo {
    pub module_id: String,
    /// The series link relative to the module's `RootURL`, as the module reports it.
    pub link: String,
    pub title: String,
    pub alt_titles: String,
    pub authors: String,
    pub artists: String,
    /// The module's comma-separated genres, split.
    pub genres: Vec<String>,
    pub status: SeriesStatus,
    pub summary: String,
    /// The cover through `/api/covers`; `None` when the module reports none.
    pub cover_url: Option<String>,
    /// In module order.
    pub chapters: Vec<ChapterInfo>,
    /// Whether the series is a favorite.
    pub in_library: bool,
}

/// One chapter of a series.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChapterInfo {
    pub name: String,
    /// Relative to the module's `RootURL`.
    pub link: String,
    /// Whether the chapter was downloaded before.
    pub downloaded: bool,
}

/// `MangaInfo.Status` (`MangaInfo_Status*`, baseunits/uBaseUnit.pas:230-233).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SeriesStatus {
    Completed,
    Ongoing,
    Hiatus,
    Cancelled,
    Unknown,
}

impl SeriesStatus {
    fn from_fmd(status: &str) -> Self {
        match status {
            "0" => Self::Completed,
            "1" => Self::Ongoing,
            "2" => Self::Hiatus,
            "3" => Self::Cancelled,
            _ => Self::Unknown,
        }
    }
}

#[utoipa::path(get, path = "/api/series", tag = "series", operation_id = "getSeries",
    params(SeriesQuery),
    responses(
        (status = 200, body = SeriesInfo, description = "The series' info and chapters"),
        (status = 404, body = Problem, description = "No such module, or the module found no series there"),
        (status = 502, body = Problem, description = "The website could not be reached"),
    ))]
pub(crate) async fn get(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<SeriesQuery>,
) -> Result<Json<SeriesInfo>, ApiError> {
    let options = InfoOptions {
        remove_manga_name_from_chapter: state.settings.get().saveto.remove_manga_name_from_chapter,
    };
    let key = (query.module.clone(), query.link.clone(), options);
    let info = match state.series_cache.get(&key) {
        Some(info) => info,
        None => {
            let info = state
                .modules
                .get_info(&query.module, &query.link, options)
                .await
                .map_err(info_error)?;
            state.series_cache.put(key, info.clone());
            info
        }
    };
    // FMD2 looks up the downloaded chapters by the link the module reports
    // (mangadownloader/forms/frmMain.pas:2207).
    let (module, link) = (query.module.clone(), info.link.clone());
    let (downloaded, favorite) = state
        .blocking(move |db| -> Result<_, ApiError> {
            let downloaded = db.downloaded_chapters().list_for(&module, &link)?;
            let favorite = db.favorites().find(&module, &link)?;
            Ok((downloaded, favorite.is_some()))
        })
        .await?;
    Ok(Json(view(&query.module, info, &downloaded, favorite)))
}

/// How long a series' info is served without asking the module again: long enough for going
/// back and forth between pages, short enough that new chapters show up soon.
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
/// Series kept at most; the oldest is dropped past it.
const CACHE_ENTRIES: usize = 64;

type CacheKey = (String, String, InfoOptions);

/// Recently fetched series info, by module, link and clean-up options.
#[derive(Default)]
pub(crate) struct InfoCache {
    entries: Mutex<HashMap<CacheKey, (Instant, MangaInfo)>>,
}

impl InfoCache {
    fn get(&self, key: &CacheKey) -> Option<MangaInfo> {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let (fetched, info) = entries.get(key)?;
        (fetched.elapsed() < CACHE_TTL).then(|| info.clone())
    }

    fn put(&self, key: CacheKey, info: MangaInfo) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|_, (fetched, _)| fetched.elapsed() < CACHE_TTL);
        if entries.len() >= CACHE_ENTRIES
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (fetched, _))| *fetched)
                .map(|(k, _)| k.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(key, (Instant::now(), info));
    }
}

fn info_error(err: InfoError) -> ApiError {
    match err {
        InfoError::UnknownModule => ApiError::Missing("no such module".into()),
        InfoError::NotFound(msg) => ApiError::Missing(msg),
        InfoError::NetProblem => ApiError::BadGateway(err.to_string()),
        InfoError::Failed(msg) => ApiError::Internal(msg),
    }
}

/// `info` with each chapter's downloaded state: FMD2 finds chapter links among the downloaded
/// ones case-insensitively (`GetDownloadedChaptersState`, baseunits/uDownloadsManager.pas:1747-1768).
fn view(module: &str, info: MangaInfo, downloaded: &[String], in_library: bool) -> SeriesInfo {
    let downloaded: HashSet<String> = downloaded.iter().map(|l| l.to_lowercase()).collect();
    SeriesInfo {
        module_id: module.to_owned(),
        cover_url: (!info.cover_link.is_empty()).then(|| cover_url(module, &info.cover_link)),
        link: info.link,
        title: info.title,
        alt_titles: info.alt_titles,
        authors: info.authors,
        artists: info.artists,
        genres: info
            .genres
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_owned)
            .collect(),
        status: SeriesStatus::from_fmd(&info.status),
        summary: info.summary,
        chapters: info
            .chapters
            .into_iter()
            .map(|c| ChapterInfo {
                downloaded: downloaded.contains(&c.link.to_lowercase()),
                name: c.name,
                link: c.link,
            })
            .collect(),
        in_library,
    }
}
