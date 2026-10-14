//! `/api/favorites`: the library (FMD2's favorites tab) and its new/missing-chapter checks.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use std::sync::Arc;

use fmd_core::download::NewDownload;
use fmd_core::favorites::{
    CheckMode, CheckScope, TagsAndFormat, auto_group, favorite_save_to, stored_tags_and_format,
    website_info,
};
use fmd_core::metadata::Metadata;
use fmd_core::settings::LibraryTitle;
use fmd_store::{Favorite, FavoriteId, LibraryGroupId, ListsDb, NewFavorite, StoreError};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::covers::series::cover_url as series_cover_url;
use crate::error::{ApiJson, ApiQuery};
use crate::folder_move::{MoveRun, SeriesMove};
use crate::series::{SeriesStatus, accepted_match, fetch_info, manga_folder};
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
    /// As the website names it; download folders and files are named after it.
    pub title: String,
    /// The title the Library shows (`appearance.library_title`): the chosen title of the
    /// series' accepted MangaBaka match, else MangaBaka's main title, else `title`.
    pub display_title: String,
    pub status: SeriesStatus,
    /// Checked by the new-chapter check; `false` for a paused favorite, which no check looks at.
    pub enabled: bool,
    pub save_to: String,
    /// The cover through `/api/covers`, or through `/api/covers/series` when none is stored
    /// (FMD2's favorites have no cover).
    pub cover_url: String,
    /// Chapters on the site at the last check (FMD2's `currentchapter`).
    pub current_chapter: u32,
    /// Chapters on the site at the last check that are not downloaded, compared by link. Until
    /// its first check with links kept (e.g. imported from FMD2), the count less the downloaded.
    pub new_chapters: u32,
    /// RFC 3339.
    pub date_added: String,
    /// RFC 3339.
    pub last_checked: Option<String>,
    /// RFC 3339: when a check last found new chapters.
    pub last_updated: Option<String>,
    /// The library group it is in; `null` is "Ungrouped".
    pub group_id: Option<i64>,
    /// The website's genres, then those of the series' accepted MangaBaka match, without
    /// duplicates ignoring case.
    pub tags: Vec<String>,
    /// The accepted MangaBaka match's type: `manga`, `manhwa`, `manhua`, `oel` or `other`;
    /// `null` without a match.
    pub format: Option<String>,
    /// Consecutive checks whose fetch from the website failed; from 3 on (`FAILING_CHECKS`) the
    /// series is failing, and checks of all favorites look at it at most daily.
    pub check_failures: u32,
    /// Why the last of them failed; `null` once a check succeeds.
    pub last_check_error: Option<String>,
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
    /// Case-insensitive part of the shown or the website's title.
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
            .filter(|f| {
                q.is_empty()
                    || f.display_title.to_lowercase().contains(&q)
                    || f.title.to_lowercase().contains(&q)
            })
            .collect(),
    ))
}

/// A series to add to the library.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddFavorite {
    pub module_id: String,
    /// The series link relative to the module's `RootURL`.
    pub link: String,
    /// The download directory; the default one when missing. The generated manga folder is added.
    pub save_to: Option<String>,
}

/// Add a series to the library (`btAddToFavoritesClick`,
/// mangadownloader/forms/frmMain.pas:2797-2846): its current chapters count as seen, so only
/// chapters added later are new. It goes in the first group whose rule matches it.
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
    let website_dir = state.website_dir(&req.module_id).await?;
    let save_to = favorite_save_to(
        &state.settings.get().saveto,
        &website,
        &website_dir,
        &info,
        req.save_to.as_deref().unwrap_or_default(),
    );
    let module_id = req.module_id.clone();
    let metadata = state.metadata.clone();
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
            db.favorites().set_info(favorite.id, &website_info(&info))?;
            let links: Vec<&str> = info.chapters.iter().map(|c| c.link.as_str()).collect();
            db.downloaded_chapters()
                .mark(&module_id, &info.link, &links)?;
            if auto_group(db, metadata.as_ref(), &[favorite.id])? > 0 {
                favorite = db.favorites().get(favorite.id)?.ok_or(ApiError::NotFound)?;
            }
            Ok(favorite)
        })
        .await?;
    if let Some(jobs) = state.metadata.clone() {
        let module_id = favorite.module_id.clone();
        tokio::task::spawn_blocking(move || jobs.favorites_changed(&module_id));
    }
    let view = view(&state, favorite).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// Fields of a favorite to change; missing ones stay.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FavoritePatch {
    pub enabled: Option<bool>,
    pub save_to: Option<String>,
    pub title: Option<String>,
    /// The library group to put it in; `null` ungroups it.
    #[serde(default, deserialize_with = "null_or_value")]
    #[schema(value_type = Option<i64>, nullable)]
    pub group_id: Option<Option<i64>>,
    /// With `save_to`: also move the chapters already downloaded from the old folder to the new
    /// one, in the background; the inbox reports the result.
    #[serde(default)]
    pub move_files: bool,
}

/// Tells a field that is `null` (`Some(None)`) from a missing one (`None`, by `default`).
pub(crate) fn null_or_value<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Change a favorite (`UpdateEnabled`, `UpdateSaveTo`, `UpdateTitle`,
/// baseunits/FavoritesDB.pas:136-160).
#[utoipa::path(patch, path = "/api/favorites/{id}", tag = "library", operation_id = "updateFavorite",
    params(("id" = i64, Path, description = "Favorite id")),
    request_body = FavoritePatch,
    responses(
        (status = 200, body = FavoriteView),
        (status = 400, body = Problem, description = "No such group"),
        (status = 404, body = Problem, description = "No such favorite"),
        (status = 409, body = Problem, description = "With `move_files`: a move is running, and nothing changed"),
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
    let run = match req.save_to {
        Some(_) if req.move_files => Some(MoveRun::reserve(&state)?),
        _ => None,
    };
    let (favorite, old_save_to) = state
        .blocking(move |db| -> Result<(Favorite, String), ApiError> {
            let mut favorite = db
                .favorites()
                .get(FavoriteId(id))?
                .ok_or(ApiError::NotFound)?;
            let old_save_to = favorite.save_to.clone();
            if let Some(enabled) = req.enabled {
                favorite.enabled = enabled;
            }
            if let Some(save_to) = req.save_to {
                favorite.save_to = save_to;
            }
            if let Some(title) = req.title {
                favorite.title = title.trim().to_owned();
            }
            if let Some(group_id) = req.group_id {
                let group_id = group_id.map(LibraryGroupId);
                if let Some(id) = group_id
                    && db.library_groups().get(id)?.is_none()
                {
                    return Err(ApiError::BadRequest(format!("no group with id {}", id.0)));
                }
                favorite.group_id = group_id;
            }
            db.favorites().update(&favorite)?;
            Ok((favorite, old_save_to))
        })
        .await?;
    if let Some(run) = run {
        run.start(vec![SeriesMove {
            id: favorite.id,
            title: favorite.title.clone(),
            from: old_save_to,
            to: favorite.save_to.clone(),
        }]);
    }
    Ok(Json(view(&state, favorite).await?))
}

/// One change to several favorites.
#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkChange {
    pub ids: Vec<i64>,
    /// Resume (`true`) or pause (`false`) them.
    pub enabled: Option<bool>,
    /// The library group to put them in; `null` ungroups them.
    #[serde(default, deserialize_with = "null_or_value")]
    #[schema(value_type = Option<i64>, nullable)]
    pub group_id: Option<Option<i64>>,
    /// A parent folder: each favorite downloads to its own manga folder in it, as
    /// `POST /api/save-folder` makes it. Chapters already downloaded are not moved unless
    /// `move_files` is set.
    pub destination: Option<String>,
    /// With `destination`: also move each favorite's chapters already downloaded from its old
    /// folder to its new one, in the background; the inbox reports the result.
    #[serde(default)]
    pub move_files: bool,
    /// Mark every chapter listed at their last check seen, as downloading them would, which
    /// clears their new chapters. No file is downloaded. A favorite not checked since it was
    /// imported from FMD2 has no chapter links to mark, so its count stays until its next check.
    #[serde(default)]
    pub mark_seen: bool,
    /// Remove them from the library instead; their downloaded chapters stay recorded. Cannot be
    /// combined with a change.
    #[serde(default)]
    pub remove: bool,
}

/// Change or remove several favorites at once, all of them or none.
#[utoipa::path(post, path = "/api/favorites/bulk", tag = "library", operation_id = "bulkUpdateFavorites",
    request_body = BulkChange,
    responses(
        (status = 200, body = Vec<FavoriteView>, description = "The changed favorites, in the order of `ids`; none after a removal"),
        (status = 400, body = Problem, description = "No such favorite or group, and nothing changed"),
        (status = 409, body = Problem, description = "With `move_files`: a move is running, and nothing changed"),
        (status = 422, body = Problem, description = "An empty destination"),
    ))]
pub(crate) async fn bulk(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<BulkChange>,
) -> Result<Json<Vec<FavoriteView>>, ApiError> {
    if req
        .destination
        .as_deref()
        .is_some_and(|d| d.trim().is_empty())
    {
        return Err(ApiError::Invalid {
            field: Some("destination".into()),
            detail: "the destination cannot be empty".into(),
        });
    }
    let ids: Vec<FavoriteId> = req.ids.iter().copied().map(FavoriteId).collect();
    let favorites = if req.remove {
        if req.enabled.is_some()
            || req.group_id.is_some()
            || req.destination.is_some()
            || req.mark_seen
            || req.move_files
        {
            return Err(ApiError::BadRequest(
                "remove cannot be combined with a change".into(),
            ));
        }
        bulk_remove(&state, ids).await?;
        Vec::new()
    } else {
        bulk_change(&state, ids, req).await?
    };
    let mut views = Vec::with_capacity(favorites.len());
    for favorite in favorites {
        views.push(view(&state, favorite).await?);
    }
    Ok(Json(views))
}

async fn bulk_remove(state: &AppState, ids: Vec<FavoriteId>) -> Result<(), ApiError> {
    state
        .blocking(move |db| -> Result<(), ApiError> {
            if !db.favorites().delete_all(&ids)? {
                return Err(ApiError::BadRequest(
                    "a favorite is not in the library".into(),
                ));
            }
            Ok(())
        })
        .await
}

/// Each favorite is read again in the transaction that stores it, so a check finishing
/// meanwhile keeps what it found.
async fn bulk_change(
    state: &AppState,
    ids: Vec<FavoriteId>,
    req: BulkChange,
) -> Result<Vec<Favorite>, ApiError> {
    let group_id = req.group_id.map(|g| g.map(LibraryGroupId));
    let read = ids.clone();
    let favorites = state
        .blocking(move |db| -> Result<Vec<Favorite>, ApiError> {
            if let Some(Some(id)) = group_id
                && db.library_groups().get(id)?.is_none()
            {
                return Err(ApiError::BadRequest(format!("no group with id {}", id.0)));
            }
            read.into_iter()
                .map(|id| {
                    db.favorites().get(id)?.ok_or_else(|| {
                        ApiError::BadRequest(format!("no favorite with id {}", id.0))
                    })
                })
                .collect()
        })
        .await?;
    let mut folders = std::collections::HashMap::new();
    if let Some(destination) = &req.destination {
        for favorite in favorites {
            // As the series page's folder dialog asks for it (FolderDialog.svelte).
            let folder = manga_folder(
                state,
                NewDownload {
                    module_id: favorite.module_id,
                    title: favorite.title,
                    save_to: destination.trim().to_owned(),
                    ..NewDownload::default()
                },
            )
            .await?;
            folders.insert(favorite.id, folder);
        }
    }
    let run = match req.destination {
        Some(_) if req.move_files => Some(MoveRun::reserve(state)?),
        _ => None,
    };
    let (enabled, mark_seen) = (req.enabled, req.mark_seen);
    let (favorites, moves) = state
        .blocking(
            move |db| -> Result<(Vec<Favorite>, Vec<SeriesMove>), ApiError> {
                let mut moves = Vec::new();
                let favorites = db
                    .favorites()
                    .update_each(&ids, |favorite| {
                        if let Some(enabled) = enabled {
                            favorite.enabled = enabled;
                        }
                        if let Some(group_id) = group_id {
                            favorite.group_id = group_id;
                        }
                        if let Some(folder) = folders.remove(&favorite.id) {
                            moves.push(SeriesMove {
                                id: favorite.id,
                                title: favorite.title.clone(),
                                from: std::mem::replace(&mut favorite.save_to, folder.clone()),
                                to: folder,
                            });
                        }
                    })?
                    .ok_or_else(|| {
                        ApiError::BadRequest("a favorite was removed while changing it".into())
                    })?;
                if mark_seen {
                    db.favorites().mark_checked_chapters_seen(&ids)?;
                }
                Ok((favorites, moves))
            },
        )
        .await?;
    if let Some(run) = run {
        run.start(moves);
    }
    Ok(favorites)
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

/// The module's name, or its ID when not loaded.
fn website(state: &AppState, module_id: &str) -> String {
    state
        .modules
        .module(module_id)
        .map_or_else(|| module_id.to_owned(), |m| m.name)
}

/// The titles the Library shows, with MangaBaka's database and the lists opened once.
struct DisplayTitles {
    choice: LibraryTitle,
    /// `None` when the website's titles are shown, or MangaBaka's database is not there.
    sources: Option<(Arc<Metadata>, ListsDb)>,
}

impl DisplayTitles {
    fn new(state: &AppState) -> Self {
        let choice = state.settings.get().appearance.library_title;
        let sources = if choice == LibraryTitle::Website {
            None
        } else {
            state
                .metadata
                .as_ref()
                .and_then(|m| m.current())
                .zip(state.lists.clone())
        };
        Self { choice, sources }
    }

    /// The website's title when the series has no accepted match or the lookup fails. Blocking.
    fn of(&self, favorite: &Favorite) -> String {
        match self.lookup(&favorite.module_id, &favorite.link) {
            Ok(Some(title)) => title,
            Ok(None) => favorite.title.clone(),
            Err(e) => {
                tracing::warn!(target: "fmd_server", "MangaBaka title of a favorite: {e}");
                favorite.title.clone()
            }
        }
    }

    /// The chosen title of the accepted match, else its main title.
    fn lookup(&self, module: &str, link: &str) -> Result<Option<String>, StoreError> {
        let Some((meta, lists)) = &self.sources else {
            return Ok(None);
        };
        let Some((id, _)) = accepted_match(lists, module, link)? else {
            return Ok(None);
        };
        let Some(series) = meta.series(id)? else {
            return Ok(None);
        };
        let chosen = match self.choice {
            LibraryTitle::English => series.english_title,
            LibraryTitle::Romanized => series.romanized_title,
            LibraryTitle::Native => series.native_title,
            LibraryTitle::MangaBaka | LibraryTitle::Website => None,
        };
        let usable = |t: &String| !t.trim().is_empty();
        Ok(chosen
            .filter(usable)
            .or_else(|| Some(series.title).filter(usable)))
    }
}

async fn views(state: &AppState) -> Result<Vec<FavoriteView>, ApiError> {
    let titles = DisplayTitles::new(state);
    let metadata = state.metadata.clone();
    let rows = state
        .blocking(move |db| -> Result<_, ApiError> {
            let favorites = db.favorites().list()?;
            let mut rows = Vec::with_capacity(favorites.len());
            for favorite in favorites {
                let new_chapters = db.favorites().new_chapter_count(&favorite)?;
                let display_title = titles.of(&favorite);
                let tagged = stored_tags_and_format(db, metadata.as_ref(), &favorite)?;
                rows.push((favorite, display_title, new_chapters, tagged));
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
        .map(|(favorite, display_title, new_chapters, tagged)| {
            let website = names
                .get(&favorite.module_id)
                .cloned()
                .unwrap_or_else(|| favorite.module_id.clone());
            to_view(favorite, website, display_title, new_chapters, tagged)
        })
        .collect())
}

async fn view(state: &AppState, favorite: Favorite) -> Result<FavoriteView, ApiError> {
    let titles = DisplayTitles::new(state);
    let metadata = state.metadata.clone();
    let counted = favorite.clone();
    let (display_title, new_chapters, tagged) = state
        .blocking(move |db| -> Result<_, ApiError> {
            Ok((
                titles.of(&counted),
                db.favorites().new_chapter_count(&counted)?,
                stored_tags_and_format(db, metadata.as_ref(), &counted)?,
            ))
        })
        .await?;
    let website = website(state, &favorite.module_id);
    Ok(to_view(
        favorite,
        website,
        display_title,
        new_chapters,
        tagged,
    ))
}

fn to_view(
    favorite: Favorite,
    website: String,
    display_title: String,
    new_chapters: u32,
    tagged: TagsAndFormat,
) -> FavoriteView {
    FavoriteView {
        tags: tagged.tags,
        format: tagged.format,
        id: favorite.id.0,
        cover_url: match favorite.cover_url.as_deref().filter(|c| !c.is_empty()) {
            Some(cover) => cover_url(&favorite.module_id, cover),
            None => series_cover_url(&favorite.module_id, &favorite.link),
        },
        status: SeriesStatus::from_fmd(&favorite.status),
        new_chapters,
        current_chapter: favorite.current_chapter,
        date_added: rfc3339_from_unix_ms(favorite.date_added),
        last_checked: favorite.date_last_checked.map(rfc3339_from_unix_ms),
        last_updated: favorite.date_last_updated.map(rfc3339_from_unix_ms),
        group_id: favorite.group_id.map(|g| g.0),
        check_failures: favorite.check_failures,
        last_check_error: favorite.last_check_error,
        module_id: favorite.module_id,
        website,
        link: favorite.link,
        title: favorite.title,
        display_title,
        enabled: favorite.enabled,
        save_to: favorite.save_to,
    }
}
