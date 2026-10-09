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

/// Find the module that handles a manga URL.
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
    /// The website's summary or, when it gives none, MangaBaka's description
    /// ([`SeriesInfo::summary_from_mangabaka`]).
    pub summary: String,
    /// Whether `summary` is MangaBaka's description, for the UI to say so.
    pub summary_from_mangabaka: bool,
    /// The format of the series' MangaBaka match (`manga`, `manhwa`, `manhua`, `oel`, `other`).
    pub format: Option<String>,
    /// The year of the series' MangaBaka match.
    pub year: Option<i64>,
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
    /// Whether the chapter was downloaded before, or marked when the series was added to the
    /// library (mangadownloader/forms/frmMain.pas:2797-2846). The UI calls it "seen".
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
    pub(crate) fn from_fmd(status: &str) -> Self {
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
    let info = fetch_info(&state, &query.module, &query.link).await?;
    // FMD2 looks up the downloaded chapters by the link the module reports
    // (mangadownloader/forms/frmMain.pas:2207).
    let mangabaka = mangabaka_metadata(&state, &query.module, [&query.link, &info.link]).await;
    let (module, link) = (query.module.clone(), info.link.clone());
    let (downloaded, favorite) = state
        .blocking(move |db| -> Result<_, ApiError> {
            let downloaded = db.downloaded_chapters().list_for(&module, &link)?;
            let favorite = db.favorites().find(&module, &link)?;
            Ok((downloaded, favorite.is_some()))
        })
        .await?;
    let mut series = view(&query.module, info, &downloaded, favorite);
    if let Some(m) = mangabaka {
        if series.summary.trim().is_empty() && !m.description.trim().is_empty() {
            series.summary = m.description;
            series.summary_from_mangabaka = true;
        }
        series.format = m.format;
        series.year = m.year;
    }
    Ok(Json(series))
}

/// What MangaBaka's database says about a series.
struct MangaBakaMetadata {
    description: String,
    format: Option<String>,
    year: Option<i64>,
}

/// The MangaBaka metadata of `module`'s series at the first of `links` with an accepted match;
/// `None` without the database or a match. A failed lookup only leaves the metadata out.
async fn mangabaka_metadata(
    state: &AppState,
    module: &str,
    links: [&String; 2],
) -> Option<MangaBakaMetadata> {
    let meta = state.metadata.as_ref()?.current()?;
    let lists = state.lists.clone()?;
    let module = module.to_owned();
    let links = links.map(String::clone);
    let found = crate::state::off_thread(move || -> Result<_, fmd_store::StoreError> {
        for link in &links {
            let Some(m) = lists.matches().get(&module, link)? else {
                continue;
            };
            let Some(id) = m.series_id.filter(|_| m.confidence.is_accepted()) else {
                continue;
            };
            let description = meta.series(id)?.map(|s| s.description).unwrap_or_default();
            return Ok(Some(MangaBakaMetadata {
                description,
                format: m.format,
                year: m.year,
            }));
        }
        Ok(None)
    })
    .await;
    match found {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => {
            tracing::warn!(target: "fmd_server", "MangaBaka metadata of a series: {e}");
            None
        }
        Err(_) => None,
    }
}

/// The info of the series at `link` from module `module`, from the cache when it is recent.
pub(crate) async fn fetch_info(
    state: &AppState,
    module: &str,
    link: &str,
) -> Result<MangaInfo, ApiError> {
    let options = InfoOptions {
        remove_manga_name_from_chapter: state.settings.get().saveto.remove_manga_name_from_chapter,
    };
    let key = (module.to_owned(), link.to_owned(), options);
    if let Some(info) = state.series_cache.get(&key) {
        return Ok(info);
    }
    let info = state
        .modules
        .get_info(module, link, options)
        .await
        .map_err(info_error)?;
    state.series_cache.put(key, info.clone());
    Ok(info)
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
        summary_from_mangabaka: false,
        format: None,
        year: None,
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

/// A download whose folder to show before it is queued.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SaveFolderRequest {
    pub module_id: String,
    pub title: String,
    #[serde(default)]
    pub authors: String,
    #[serde(default)]
    pub artists: String,
    /// The folder the user picked; the website's or the default destination when empty.
    #[serde(default)]
    pub save_to: String,
}

/// The folder a download is saved in.
#[derive(Debug, Serialize, ToSchema)]
pub struct SaveFolder {
    pub folder: String,
}

/// The folder a download of the series would be saved in with the saved settings, the manga
/// folder included: what `POST /api/tasks` stores as the task's `save_to`
/// (mangadownloader/forms/frmMain.pas:2685-2710).
#[utoipa::path(post, path = "/api/save-folder", tag = "series", operation_id = "saveFolder",
    request_body = SaveFolderRequest,
    responses(
        (status = 200, body = SaveFolder),
        (status = 400, description = "Malformed body", body = Problem),
    ))]
pub(crate) async fn save_folder(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<SaveFolderRequest>,
) -> Result<Json<SaveFolder>, ApiError> {
    let website = state
        .modules
        .module(&req.module_id)
        .map_or_else(|| req.module_id.clone(), |m| m.name);
    let website_dir = state.website_dir(&req.module_id).await?;
    let download = fmd_core::download::NewDownload {
        module_id: req.module_id,
        title: req.title,
        authors: req.authors,
        artists: req.artists,
        save_to: req.save_to,
        ..Default::default()
    };
    let folder = fmd_core::download::save_to(
        &state.settings.get().saveto,
        &website,
        &website_dir,
        &download,
    );
    Ok(Json(SaveFolder { folder }))
}
