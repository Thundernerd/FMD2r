//! `/api/favorites`: the library (FMD2's favorites tab), and starting its new-chapter and
//! missing-chapter checks. The check's progress is the `favorites` job (`GET /api/jobs/favorites`)
//! and the `job.favorites.*` events.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::favorites::{CheckMode, CheckScope, favorite_save_to};
use fmd_store::{Favorite, FavoriteId, NewFavorite};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiJson, ApiQuery};
use crate::series::{SeriesStatus, fetch_info};
use crate::services::FavoritesJobs;
use crate::time::rfc3339_from_unix_ms;
use crate::{ApiError, AppState, Problem, cover_url};

/// A favorite as the Library shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct FavoriteView {
    pub id: i64,
    pub module_id: String,
    /// The module's name; its ID when the module is not loaded.
    pub website: String,
    /// The series link relative to the module's `RootURL`.
    pub link: String,
    pub title: String,
    pub status: SeriesStatus,
    /// Checked by the new-chapter check.
    pub enabled: bool,
    pub save_to: String,
    /// The cover through `/api/covers`.
    pub cover_url: Option<String>,
    /// Chapters on the site at the last check (FMD2's `currentchapter`).
    pub current_chapter: u32,
    /// Chapters on the site at the last check that are not downloaded: the chapter count less
    /// the downloaded ones, as only the count of the site's list is stored. Downloaded chapters
    /// the site no longer lists make it an undercount; a check finds the real ones by link.
    pub new_chapters: u32,
    /// RFC 3339.
    pub date_added: String,
    /// RFC 3339.
    pub last_checked: Option<String>,
    /// RFC 3339: when a check last found new chapters.
    pub last_updated: Option<String>,
}

/// Which favorites to list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FavoriteFilter {
    #[default]
    All,
    /// With chapters not downloaded.
    New,
    Ongoing,
    Completed,
    Disabled,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListQuery {
    #[serde(default)]
    #[param(inline)]
    filter: FavoriteFilter,
    /// Only this module's favorites.
    module: Option<String>,
    /// Case-insensitive part of the title.
    q: Option<String>,
}

/// The library in display order.
#[utoipa::path(get, path = "/api/favorites", tag = "library", operation_id = "listFavorites",
    params(ListQuery),
    responses((status = 200, body = Vec<FavoriteView>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> Result<Json<Vec<FavoriteView>>, ApiError> {
    let views = views(&state).await?;
    let q = query
        .q
        .as_deref()
        .map(str::to_lowercase)
        .unwrap_or_default();
    Ok(Json(
        views
            .into_iter()
            .filter(|f| match query.filter {
                FavoriteFilter::All => true,
                FavoriteFilter::New => f.new_chapters > 0,
                FavoriteFilter::Ongoing => f.status == SeriesStatus::Ongoing,
                FavoriteFilter::Completed => f.status == SeriesStatus::Completed,
                FavoriteFilter::Disabled => !f.enabled,
            })
            .filter(|f| query.module.as_ref().is_none_or(|m| *m == f.module_id))
            .filter(|f| q.is_empty() || f.title.to_lowercase().contains(&q))
            .collect(),
    ))
}

/// A series to add to the library.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddFavorite {
    pub module_id: String,
    /// The series link relative to the module's `RootURL`.
    pub link: String,
    /// The download directory; the default one when missing. The manga folder is added when
    /// generated.
    pub save_to: Option<String>,
}

/// Add a series to the library (`btAddToFavoritesClick`,
/// mangadownloader/forms/frmMain.pas:2797-2846): its current chapters count as seen, so only
/// chapters added later are new.
#[utoipa::path(post, path = "/api/favorites", tag = "library", operation_id = "addFavorite",
    request_body = AddFavorite,
    responses(
        (status = 201, body = FavoriteView),
        (status = 404, body = Problem, description = "No such module, or the module found no series there"),
        (status = 409, body = Problem, description = "The series is in the library already"),
        (status = 502, body = Problem, description = "The website could not be reached"),
    ))]
pub(crate) async fn add(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<AddFavorite>,
) -> Result<(StatusCode, Json<FavoriteView>), ApiError> {
    let info = fetch_info(&state, &req.module_id, &req.link).await?;
    let website = website(&state, &req.module_id);
    let save_to = favorite_save_to(
        &state.settings.get().saveto,
        &website,
        &info,
        req.save_to.as_deref().unwrap_or_default(),
    );
    let module_id = req.module_id.clone();
    let favorite = state
        .blocking(move |db| -> Result<Favorite, ApiError> {
            if db.favorites().find(&module_id, &info.link)?.is_some() {
                return Err(ApiError::Conflict(format!(
                    "{} is in the library already",
                    info.title
                )));
            }
            let mut favorite = db.favorites().create(&NewFavorite {
                module_id: module_id.clone(),
                link: info.link.clone(),
                title: info.title.clone(),
                save_to,
                cover_url: (!info.cover_link.is_empty()).then(|| info.cover_link.clone()),
            })?;
            favorite.status = info.status.clone();
            favorite.current_chapter = u32::try_from(info.chapters.len()).unwrap_or(u32::MAX);
            db.favorites().update(&favorite)?;
            let links: Vec<&str> = info.chapters.iter().map(|c| c.link.as_str()).collect();
            db.downloaded_chapters()
                .mark(&module_id, &info.link, &links)?;
            Ok(favorite)
        })
        .await?;
    let view = view(&state, favorite).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// Fields of a favorite to change; missing ones stay.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FavoritePatch {
    pub enabled: Option<bool>,
    pub save_to: Option<String>,
    pub title: Option<String>,
}

/// Change a favorite (`UpdateEnabled`, `UpdateSaveTo`, `UpdateTitle`,
/// baseunits/FavoritesDB.pas:136-160).
#[utoipa::path(patch, path = "/api/favorites/{id}", tag = "library", operation_id = "updateFavorite",
    params(("id" = i64, Path, description = "Favorite id")),
    request_body = FavoritePatch,
    responses(
        (status = 200, body = FavoriteView),
        (status = 404, body = Problem, description = "No such favorite"),
    ))]
pub(crate) async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(req): ApiJson<FavoritePatch>,
) -> Result<Json<FavoriteView>, ApiError> {
    if req.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
        return Err(ApiError::Invalid {
            field: Some("title".into()),
            detail: "the title cannot be empty".into(),
        });
    }
    let favorite = state
        .blocking(move |db| -> Result<Favorite, ApiError> {
            let mut favorite = db
                .favorites()
                .get(FavoriteId(id))?
                .ok_or(ApiError::NotFound)?;
            if let Some(enabled) = req.enabled {
                favorite.enabled = enabled;
            }
            if let Some(save_to) = req.save_to {
                favorite.save_to = save_to;
            }
            if let Some(title) = req.title {
                favorite.title = title.trim().to_owned();
            }
            db.favorites().update(&favorite)?;
            Ok(favorite)
        })
        .await?;
    Ok(Json(view(&state, favorite).await?))
}

/// Remove a favorite from the library; its downloaded chapters stay recorded.
#[utoipa::path(delete, path = "/api/favorites/{id}", tag = "library", operation_id = "deleteFavorite",
    params(("id" = i64, Path, description = "Favorite id")),
    responses(
        (status = 204, description = "Removed"),
        (status = 404, body = Problem, description = "No such favorite"),
    ))]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .blocking(move |db| -> Result<(), ApiError> {
            db.favorites()
                .get(FavoriteId(id))?
                .ok_or(ApiError::NotFound)?;
            db.favorites().delete(FavoriteId(id))?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Which favorites to check.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CheckRequest {
    /// These favorites; every enabled favorite when missing.
    pub ids: Option<Vec<i64>>,
}

/// Check enabled favorites for new chapters now (`CheckForNewChapter`,
/// baseunits/uFavoritesManager.pas:832-881). The body is optional.
#[utoipa::path(post, path = "/api/favorites/check", tag = "library", operation_id = "checkFavorites",
    request_body(content = Option<CheckRequest>),
    responses(
        (status = 202, description = "Started; progress follows as `job.favorites.*` events"),
        (status = 409, body = Problem, description = "A check is running"),
        (status = 503, body = Problem, description = "The favorites check is not running in this server"),
    ))]
pub(crate) async fn check(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let req: CheckRequest = if body.iter().all(u8::is_ascii_whitespace) {
        CheckRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?
    };
    let scope = match req.ids {
        Some(ids) => CheckScope::Only(ids.into_iter().map(FavoriteId).collect()),
        None => CheckScope::All,
    };
    start(&state, scope, CheckMode::New)
}

/// Check a favorite for chapters missing from its directory (`CheckForMissingChapters`,
/// baseunits/uFavoritesManager.pas:883-928), to download them again.
#[utoipa::path(post, path = "/api/favorites/{id}/check-missing", tag = "library",
    operation_id = "checkMissingChapters",
    params(("id" = i64, Path, description = "Favorite id")),
    responses(
        (status = 202, description = "Started; progress follows as `job.favorites.*` events"),
        (status = 409, body = Problem, description = "A check is running"),
        (status = 503, body = Problem, description = "The favorites check is not running in this server"),
    ))]
pub(crate) async fn check_missing(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    start(
        &state,
        CheckScope::Only(vec![FavoriteId(id)]),
        CheckMode::Missing,
    )
}

fn start(state: &AppState, scope: CheckScope, mode: CheckMode) -> Result<StatusCode, ApiError> {
    let jobs: &dyn FavoritesJobs = state
        .favorites
        .as_deref()
        .ok_or_else(|| ApiError::Unavailable("the favorites check is not running".into()))?;
    jobs.check(scope, mode)?;
    Ok(StatusCode::ACCEPTED)
}

/// The module's name, or its ID when it is not loaded.
fn website(state: &AppState, module_id: &str) -> String {
    state
        .modules
        .module(module_id)
        .map_or_else(|| module_id.to_owned(), |m| m.name)
}

/// Every favorite as the Library shows it.
async fn views(state: &AppState) -> Result<Vec<FavoriteView>, ApiError> {
    let rows = state
        .blocking(|db| -> Result<_, ApiError> {
            let favorites = db.favorites().list()?;
            let mut rows = Vec::with_capacity(favorites.len());
            for favorite in favorites {
                let downloaded = db
                    .downloaded_chapters()
                    .count_for(&favorite.module_id, &favorite.link)?;
                rows.push((favorite, downloaded));
            }
            Ok(rows)
        })
        .await?;
    let names: std::collections::HashMap<String, String> = state
        .modules
        .modules()
        .into_iter()
        .map(|m| (m.id, m.name))
        .collect();
    Ok(rows
        .into_iter()
        .map(|(favorite, downloaded)| {
            let website = names
                .get(&favorite.module_id)
                .cloned()
                .unwrap_or_else(|| favorite.module_id.clone());
            to_view(favorite, website, downloaded)
        })
        .collect())
}

async fn view(state: &AppState, favorite: Favorite) -> Result<FavoriteView, ApiError> {
    let (module, link) = (favorite.module_id.clone(), favorite.link.clone());
    let downloaded = state
        .blocking(move |db| db.downloaded_chapters().count_for(&module, &link))
        .await?;
    let website = website(state, &favorite.module_id);
    Ok(to_view(favorite, website, downloaded))
}

fn to_view(favorite: Favorite, website: String, downloaded: u32) -> FavoriteView {
    FavoriteView {
        id: favorite.id.0,
        cover_url: favorite
            .cover_url
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(|c| cover_url(&favorite.module_id, c)),
        status: SeriesStatus::from_fmd(&favorite.status),
        new_chapters: favorite.current_chapter.saturating_sub(downloaded),
        current_chapter: favorite.current_chapter,
        date_added: rfc3339_from_unix_ms(favorite.date_added),
        last_checked: favorite.date_last_checked.map(rfc3339_from_unix_ms),
        last_updated: favorite.date_last_updated.map(rfc3339_from_unix_ms),
        module_id: favorite.module_id,
        website,
        link: favorite.link,
        title: favorite.title,
        enabled: favorite.enabled,
        save_to: favorite.save_to,
    }
}
