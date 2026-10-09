//! Discover: searching every module's manga list (`/api/lists/search`, `/api/lists/facets`) and
//! starting list updates and FMD2-DB imports (`/api/lists/{module}/...`).

use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::lists::{ListJobKind, ListJobs};
use fmd_store::{FacetCount, MasterListEntry, PageRequest, SearchFilters};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::ApiQuery;
use crate::state::off_thread;
use crate::{ApiError, AppState, Problem};

/// Results per page when the request names none.
const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_PAGE_SIZE: u32 = 200;

/// The search and filters of the Discover page.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SearchQuery {
    /// Only this module's list; the selected websites' (`general.selected_websites`) when
    /// absent.
    module: Option<String>,
    /// Words that must each start a word of the title or an alternative title.
    q: Option<String>,
    /// Comma-separated genres that must all occur.
    genres_include: Option<String>,
    /// Comma-separated genres none of which may occur.
    genres_exclude: Option<String>,
    /// Exact status: `0` completed, `1` ongoing, `2` hiatus, `3` cancelled
    /// (`MangaInfo_Status*`, baseunits/uBaseUnit.pas:230-233).
    status: Option<String>,
    /// 1-based page number.
    #[param(minimum = 1)]
    page: Option<u32>,
    /// Results per page, 50 by default.
    #[param(minimum = 1, maximum = 200)]
    page_size: Option<u32>,
}

/// The search of the facets.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct FacetsQuery {
    /// Only this module's list; the selected websites' (`general.selected_websites`) when
    /// absent.
    module: Option<String>,
    /// As in `/api/lists/search`.
    q: Option<String>,
}

/// One title of a module's list.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListItem {
    pub module_id: String,
    /// The title's link without the module's host, as the series page takes it.
    pub link: String,
    pub title: String,
    pub alttitles: String,
    pub authors: String,
    pub artists: String,
    pub genres: Vec<String>,
    /// As in the `status` filter; empty when unknown.
    pub status: String,
    pub numchapter: u32,
    /// Julian day number of the day the title was first listed.
    pub added_jdn: i64,
}

impl From<MasterListEntry> for ListItem {
    fn from(entry: MasterListEntry) -> Self {
        let l = entry.listing;
        ListItem {
            module_id: entry.module_id,
            link: l.link,
            title: l.title,
            alttitles: l.alttitles,
            authors: l.authors,
            artists: l.artists,
            genres: split_genres(&l.genres),
            status: l.status,
            numchapter: l.numchapter,
            added_jdn: l.added_jdn,
        }
    }
}

/// One page of search results.
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchPage {
    pub items: Vec<ListItem>,
    /// Matches across all pages.
    pub total: u64,
    /// 1-based.
    pub page: u32,
    pub page_size: u32,
}

/// How many matching titles carry a genre or status.
#[derive(Debug, Serialize, ToSchema)]
pub struct FacetValue {
    pub value: String,
    pub count: u64,
}

impl From<FacetCount> for FacetValue {
    fn from(f: FacetCount) -> Self {
        FacetValue {
            value: f.value,
            count: f.count,
        }
    }
}

/// The genres and statuses of the titles a search matches, most common first.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListFacets {
    pub genres: Vec<FacetValue>,
    pub statuses: Vec<FacetValue>,
}

/// A list job that was started.
#[derive(Debug, Serialize, ToSchema)]
pub struct ListJobStarted {
    pub module_id: String,
    pub job: ListJobKind,
}

/// FMD2's comma-separated genre list as items.
fn split_genres(genres: &str) -> Vec<String> {
    genres
        .split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The modules a search covers: `module` when given, otherwise the selected websites
/// (`general.selected_websites`) that are loaded, as FMD2's "all websites" search covers only
/// `SitesList` (baseunits/DBDataProcess.pas:649-683, :1459). `None` when that is no module.
fn filters(state: &AppState, module: Option<String>) -> Option<SearchFilters> {
    let module_ids: Vec<String> = match module.filter(|m| !m.is_empty()) {
        Some(module) => vec![module],
        None => {
            let loaded: HashSet<String> =
                state.modules.modules().into_iter().map(|m| m.id).collect();
            let mut selected = state.settings.get().general.selected_websites.clone();
            selected.retain(|id| loaded.contains(id));
            selected
        }
    };
    (!module_ids.is_empty()).then(|| SearchFilters {
        module_ids,
        ..SearchFilters::default()
    })
}

/// Search the manga lists.
#[utoipa::path(get, path = "/api/lists/search", tag = "lists", operation_id = "searchLists",
    params(SearchQuery),
    responses(
        (status = 200, body = SearchPage, description = "One page of matches, by title"),
        (status = 503, description = "No lists.db", body = Problem),
    ))]
pub(crate) async fn search(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<SearchQuery>,
) -> Result<Json<SearchPage>, ApiError> {
    let lists = state.lists()?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let Some(filters) = filters(&state, query.module) else {
        return Ok(Json(SearchPage {
            items: Vec::new(),
            total: 0,
            page,
            page_size,
        }));
    };
    let filters = SearchFilters {
        include_genres: query
            .genres_include
            .as_deref()
            .map(split_genres)
            .unwrap_or_default(),
        exclude_genres: query
            .genres_exclude
            .as_deref()
            .map(split_genres)
            .unwrap_or_default(),
        status: query.status.filter(|s| !s.is_empty()),
        ..filters
    };
    let request = PageRequest {
        offset: (page - 1).saturating_mul(page_size),
        limit: page_size,
    };
    let q = query.q.unwrap_or_default();
    let results = off_thread(move || lists.masterlist().search(&q, &filters, request)).await??;
    Ok(Json(SearchPage {
        items: results.entries.into_iter().map(ListItem::from).collect(),
        total: results.total,
        page,
        page_size,
    }))
}

/// Genre and status counts of the titles a search matches.
#[utoipa::path(get, path = "/api/lists/facets", tag = "lists", operation_id = "listFacets",
    params(FacetsQuery),
    responses(
        (status = 200, body = ListFacets, description = "Genre and status counts"),
        (status = 503, description = "No lists.db", body = Problem),
    ))]
pub(crate) async fn facets(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<FacetsQuery>,
) -> Result<Json<ListFacets>, ApiError> {
    let lists = state.lists()?;
    let Some(filters) = filters(&state, query.module) else {
        return Ok(Json(ListFacets {
            genres: Vec::new(),
            statuses: Vec::new(),
        }));
    };
    let q = query.q.unwrap_or_default();
    let facets = off_thread(move || lists.masterlist().facets(&q, &filters)).await??;
    Ok(Json(ListFacets {
        genres: facets.genres.into_iter().map(FacetValue::from).collect(),
        statuses: facets.statuses.into_iter().map(FacetValue::from).collect(),
    }))
}

/// Update a module's list by running its update-list callbacks.
#[utoipa::path(post, path = "/api/lists/{module}/update", tag = "lists",
    operation_id = "updateList",
    params(("module" = String, Path, description = "Module ID")),
    responses(
        (status = 202, body = ListJobStarted,
            description = "Started; progress follows as `job.lists.*` events"),
        (status = 404, description = "No module with that ID is loaded", body = Problem),
        (status = 409, description = "A list job of the module is already running", body = Problem),
        (status = 503, description = "List jobs are not available", body = Problem),
    ))]
pub(crate) async fn update(
    State(state): State<AppState>,
    Path(module): Path<String>,
) -> Result<(StatusCode, Json<ListJobStarted>), ApiError> {
    start(&state, module, ListJobKind::Update, ListJobs::update).await
}

/// Replace a module's list with its FMD2-DB dump (`update_lists.db_url`).
#[utoipa::path(post, path = "/api/lists/{module}/import-db", tag = "lists",
    operation_id = "importListDb",
    params(("module" = String, Path, description = "Module ID")),
    responses(
        (status = 202, body = ListJobStarted,
            description = "Started; progress follows as `job.lists.*` events"),
        (status = 404, description = "No module with that ID is loaded", body = Problem),
        (status = 409, description = "A list job of the module is already running", body = Problem),
        (status = 503, description = "List jobs are not available", body = Problem),
    ))]
pub(crate) async fn import_db(
    State(state): State<AppState>,
    Path(module): Path<String>,
) -> Result<(StatusCode, Json<ListJobStarted>), ApiError> {
    start(&state, module, ListJobKind::ImportDb, ListJobs::import_db).await
}

/// Stop a module's running list job.
#[utoipa::path(post, path = "/api/lists/{module}/cancel", tag = "lists",
    operation_id = "cancelListJob",
    params(("module" = String, Path, description = "Module ID")),
    responses(
        (status = 202, description = "Cancelling; a `job.lists.cancelled` event follows"),
        (status = 409, description = "No list job of the module is running", body = Problem),
        (status = 503, description = "List jobs are not available", body = Problem),
    ))]
pub(crate) async fn cancel(
    State(state): State<AppState>,
    Path(module): Path<String>,
) -> Result<StatusCode, ApiError> {
    list_jobs(&state)?.cancel(&module)?;
    Ok(StatusCode::ACCEPTED)
}

fn list_jobs(state: &AppState) -> Result<ListJobs, ApiError> {
    state
        .list_jobs
        .clone()
        .ok_or_else(|| ApiError::Unavailable("list jobs are not available".into()))
}

/// Starts `job` of `module` with `action` off the async threads (it spawns a thread).
async fn start(
    state: &AppState,
    module: String,
    job: ListJobKind,
    action: fn(&ListJobs, &str) -> Result<(), fmd_core::lists::ListJobError>,
) -> Result<(StatusCode, Json<ListJobStarted>), ApiError> {
    let jobs = list_jobs(state)?;
    let module = off_thread(move || action(&jobs, &module).map(|()| module)).await??;
    Ok((
        StatusCode::ACCEPTED,
        Json(ListJobStarted {
            module_id: module,
            job,
        }),
    ))
}
