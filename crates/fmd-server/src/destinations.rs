//! `/api/destinations`: repointing what downloads to a destination when its folder changes
//! (#284).

use std::path::Path;

use axum::Json;
use axum::extract::State;
use fmd_store::Favorite;
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
/// the innermost one.
#[utoipa::path(post, path = "/api/destinations/relocate", tag = "settings",
    operation_id = "relocateDestinations",
    request_body = RelocateRequest,
    responses(
        (status = 200, body = Vec<RelocationCounts>, description = "What was repointed, in the order of `relocations`"),
        (status = 400, body = Problem, description = "Malformed body"),
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
                let mut ids = Vec::new();
                for favorite in db.favorites().list()? {
                    if let Some(i) = containing(&relocations, &favorite.save_to) {
                        counts[i].series += 1;
                        ids.push(favorite.id);
                    }
                }
                let repo = db.module_settings();
                let mut websites = Vec::new();
                for module in repo.module_ids()? {
                    let Some(stored) = repo.get(&module)? else {
                        continue;
                    };
                    let folder = Path::new(stored.save_to.trim());
                    if stored.save_to.trim().is_empty() {
                        continue;
                    }
                    if let Some(i) = relocations
                        .iter()
                        .position(|r| folder == Path::new(&r.from))
                    {
                        counts[i].websites += 1;
                        websites.push((stored, relocations[i].to.clone()));
                    }
                }
                let mut moves = Vec::new();
                if dry_run {
                    return Ok((counts, moves));
                }
                db.favorites()
                    .update_each(&ids, |favorite: &mut Favorite| {
                        let Some(i) = containing(&relocations, &favorite.save_to) else {
                            return;
                        };
                        let relocation = &relocations[i];
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
                    .ok_or_else(|| {
                        ApiError::Conflict("a series was removed while repointing it".into())
                    })?;
                for (mut stored, to) in websites {
                    stored.save_to = to;
                    repo.upsert(&stored)?;
                }
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

/// The relocation whose old folder is, or holds, `folder`: the innermost when several do.
fn containing(relocations: &[Relocation], folder: &str) -> Option<usize> {
    relocations
        .iter()
        .enumerate()
        .filter(|(_, r)| Path::new(folder).starts_with(&r.from))
        .max_by_key(|(_, r)| Path::new(&r.from).components().count())
        .map(|(i, _)| i)
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
