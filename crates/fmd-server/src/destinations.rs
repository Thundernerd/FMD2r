//! `/api/destinations`: repointing what downloads to a destination when its folder changes
//! (#284).

use std::collections::HashMap;
use std::path::Path;

use axum::Json;
use axum::extract::State;
use fmd_store::{AppDb, Favorite, FavoriteId, ModuleSettings};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::folder_move::{MoveRun, SeriesMove};
use crate::{ApiError, AppState, Problem};

/// A destination's folder that changed.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct Relocation {
    /// The destination's old folder.
    pub from: String,
    /// Its new folder.
    pub to: String,
}

/// Destinations whose folders changed, repointed together so their files move in one run of the
/// move job.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RelocateRequest {
    pub relocations: Vec<Relocation>,
    /// Also move the repointed series' chapters already downloaded to their new folders, in the
    /// background; the inbox reports the result. A series that downloads straight into an old
    /// folder is repointed without moving anything, as that would move the whole destination.
    #[serde(default)]
    pub move_files: bool,
    /// Only count what would be repointed; nothing changes.
    #[serde(default)]
    pub dry_run: bool,
}

/// What a relocation repointed, or would.
#[derive(Debug, Serialize, ToSchema)]
pub struct RelocationCounts {
    pub from: String,
    pub to: String,
    /// Library series whose folder is the old folder or inside it.
    pub series: usize,
    /// Websites whose own destination is the old folder.
    pub websites: usize,
}

/// Point the library series and websites that download to destinations' old folders to their new
/// ones. A series' folder `from/A/B` becomes `to/A/B`; `from` matches by whole path components,
/// so `/data/manga` does not match `/data/manga2`. A series inside several old folders follows
/// the innermost one; one inside another destination within the old folder stays.
#[utoipa::path(post, path = "/api/destinations/relocate", tag = "settings",
    operation_id = "relocateDestinations",
    request_body = RelocateRequest,
    responses(
        (status = 200, body = Vec<RelocationCounts>, description = "What was repointed, in the order of `relocations`"),
        (status = 400, body = Problem, description = "Malformed body, or a series was removed meanwhile; nothing changed"),
        (status = 409, body = Problem, description = "With `move_files`: a move is running, and nothing changed"),
        (status = 422, body = Problem, description = "An empty folder"),
    ))]
pub(crate) async fn relocate(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<RelocateRequest>,
) -> Result<Json<Vec<RelocationCounts>>, ApiError> {
    for (i, relocation) in req.relocations.iter().enumerate() {
        for (field, folder) in [("from", &relocation.from), ("to", &relocation.to)] {
            if folder.trim().is_empty() {
                return Err(ApiError::Invalid {
                    field: Some(format!("relocations.{i}.{field}")),
                    detail: "the folder cannot be empty".into(),
                });
            }
        }
    }
    let relocations: Vec<Relocation> = req
        .relocations
        .into_iter()
        .map(|r| Relocation {
            from: r.from.trim().to_owned(),
            to: r.to.trim().to_owned(),
        })
        .collect();
    let destinations: Vec<String> = state
        .settings
        .get()
        .saveto
        .destinations
        .iter()
        .map(|d| d.path.trim().to_owned())
        .collect();
    let dry_run = req.dry_run;
    let run = if req.move_files && !dry_run {
        Some(MoveRun::reserve(&state)?)
    } else {
        None
    };
    let (counts, moves) = state
        .blocking(
            move |db| -> Result<(Vec<RelocationCounts>, Vec<SeriesMove>), ApiError> {
                let mut counts: Vec<RelocationCounts> = relocations
                    .iter()
                    .map(|r| RelocationCounts {
                        from: r.from.clone(),
                        to: r.to.clone(),
                        series: 0,
                        websites: 0,
                    })
                    .collect();
                let mut series = HashMap::new();
                for favorite in db.favorites().list()? {
                    if let Some(i) = containing(&relocations, &destinations, &favorite.save_to) {
                        counts[i].series += 1;
                        series.insert(favorite.id, i);
                    }
                }
                let websites = websites_in(db, &relocations)?;
                for (_, i) in &websites {
                    counts[*i].websites += 1;
                }
                if dry_run {
                    return Ok((counts, Vec::new()));
                }
                let moves = repoint(db, &relocations, &series, websites)?;
                Ok((counts, moves))
            },
        )
        .await?;
    if let Some(run) = run
        && !moves.is_empty()
    {
        run.start(moves);
    }
    Ok(Json(counts))
}

/// The websites whose own destination is an old folder, with the index of its relocation.
fn websites_in(
    db: &AppDb,
    relocations: &[Relocation],
) -> Result<Vec<(ModuleSettings, usize)>, ApiError> {
    let repo = db.module_settings();
    let mut websites = Vec::new();
    for module in repo.module_ids()? {
        let Some(stored) = repo.get(&module)? else {
            continue;
        };
        let folder = stored.save_to.trim();
        if folder.is_empty() {
            continue;
        }
        if let Some(i) = relocations
            .iter()
            .position(|r| Path::new(folder) == Path::new(&r.from))
        {
            websites.push((stored, i));
        }
    }
    Ok(websites)
}

/// Points `series` (by the index of their relocation) and `websites` to the new folders; returns
/// the series whose downloads move along. A series that downloads straight into an old folder
/// does not move, as that would move the whole destination.
fn repoint(
    db: &AppDb,
    relocations: &[Relocation],
    series: &HashMap<FavoriteId, usize>,
    websites: Vec<(ModuleSettings, usize)>,
) -> Result<Vec<SeriesMove>, ApiError> {
    let ids: Vec<FavoriteId> = series.keys().copied().collect();
    let mut moves = Vec::new();
    db.favorites()
        .update_each(&ids, |favorite: &mut Favorite| {
            let Some(relocation) = series.get(&favorite.id).map(|&i| &relocations[i]) else {
                return;
            };
            let save_to = relocated(&favorite.save_to, relocation);
            let old = std::mem::replace(&mut favorite.save_to, save_to.clone());
            if Path::new(&old) != Path::new(&relocation.from) {
                moves.push(SeriesMove {
                    id: favorite.id,
                    title: favorite.title.clone(),
                    from: old,
                    to: save_to,
                });
            }
        })?
        .ok_or_else(|| ApiError::BadRequest("a series was removed while repointing it".into()))?;
    let repo = db.module_settings();
    for (mut stored, i) in websites {
        stored.save_to.clone_from(&relocations[i].to);
        repo.upsert(&stored)?;
    }
    Ok(moves)
}

/// The relocation whose old folder is, or holds, `folder`: the innermost when several do. A
/// folder inside another of `destinations` within that old folder keeps its place: that
/// destination did not change.
fn containing(relocations: &[Relocation], destinations: &[String], folder: &str) -> Option<usize> {
    let folder = Path::new(folder);
    let (i, relocation) = relocations
        .iter()
        .enumerate()
        .filter(|(_, r)| folder.starts_with(&r.from))
        .max_by_key(|(_, r)| Path::new(&r.from).components().count())?;
    let from = Path::new(&relocation.from);
    let kept = destinations
        .iter()
        .map(Path::new)
        .any(|d| d != from && d.starts_with(from) && folder.starts_with(d));
    (!kept).then_some(i)
}

/// `folder`, inside `relocation.from`, at the same place under `relocation.to`.
fn relocated(folder: &str, relocation: &Relocation) -> String {
    match Path::new(folder).strip_prefix(&relocation.from) {
        Ok(rest) if !rest.as_os_str().is_empty() => Path::new(&relocation.to)
            .join(rest)
            .to_string_lossy()
            .into_owned(),
        _ => relocation.to.clone(),
    }
}
