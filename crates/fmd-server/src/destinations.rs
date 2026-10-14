//! `/api/destinations`: repointing what downloads to a destination when its folder changes
//! (#284).

use std::path::{Path, PathBuf};

use axum::Json;
use axum::extract::State;
use fmd_store::Favorite;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ApiJson;
use crate::folder_move::{MoveRun, SeriesMove};
use crate::{ApiError, AppState, Problem};

/// A destination's folder that changed.
#[derive(Debug, Deserialize, ToSchema)]
pub struct Relocation {
    /// The destination's old folder.
    pub from: String,
    /// Its new folder.
    pub to: String,
    /// Also move the repointed series' chapters already downloaded to their new folders, in the
    /// background; the inbox reports the result. A series that downloads straight into the old
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
    /// Library series whose folder is the old folder or inside it.
    pub series: usize,
    /// Websites whose own destination is the old folder.
    pub websites: usize,
}

/// Point the library series and websites that download to a destination's old folder to its
/// new one. A series' folder `from/A/B` becomes `to/A/B`; `from` matches by whole path
/// components, so `/data/manga` does not match `/data/manga2`.
#[utoipa::path(post, path = "/api/destinations/relocate", tag = "settings",
    operation_id = "relocateDestination",
    request_body = Relocation,
    responses(
        (status = 200, body = RelocationCounts, description = "What was repointed"),
        (status = 400, body = Problem, description = "Malformed body"),
        (status = 409, body = Problem, description = "With `move_files`: a move is running, and nothing changed"),
        (status = 422, body = Problem, description = "An empty folder"),
    ))]
pub(crate) async fn relocate(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<Relocation>,
) -> Result<Json<RelocationCounts>, ApiError> {
    for (field, folder) in [("from", &req.from), ("to", &req.to)] {
        if folder.trim().is_empty() {
            return Err(ApiError::Invalid {
                field: Some(field.into()),
                detail: "the folder cannot be empty".into(),
            });
        }
    }
    let (from, to) = (PathBuf::from(req.from.trim()), req.to.trim().to_owned());
    let dry_run = req.dry_run;
    let run = if req.move_files && !dry_run {
        Some(MoveRun::reserve(&state)?)
    } else {
        None
    };
    let (counts, moves) = state
        .blocking(
            move |db| -> Result<(RelocationCounts, Vec<SeriesMove>), ApiError> {
                let ids: Vec<_> = db
                    .favorites()
                    .list()?
                    .into_iter()
                    .filter(|f| relocated(&f.save_to, &from, &to).is_some())
                    .map(|f| f.id)
                    .collect();
                let repo = db.module_settings();
                let mut websites = Vec::new();
                for module in repo.module_ids()? {
                    if let Some(stored) = repo.get(&module)?
                        && !stored.save_to.trim().is_empty()
                        && Path::new(stored.save_to.trim()) == from
                    {
                        websites.push(stored);
                    }
                }
                let counts = RelocationCounts {
                    series: ids.len(),
                    websites: websites.len(),
                };
                let mut moves = Vec::new();
                if dry_run {
                    return Ok((counts, moves));
                }
                db.favorites()
                    .update_each(&ids, |favorite: &mut Favorite| {
                        let Some(save_to) = relocated(&favorite.save_to, &from, &to) else {
                            return;
                        };
                        let old = std::mem::replace(&mut favorite.save_to, save_to.clone());
                        if Path::new(&old) != from {
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
                for mut stored in websites {
                    stored.save_to.clone_from(&to);
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

/// `folder` moved from under `from` to the same place under `to`; `None` when it is not `from`
/// or inside it.
fn relocated(folder: &str, from: &Path, to: &str) -> Option<String> {
    let rest = Path::new(folder).strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_owned()
    } else {
        Path::new(to).join(rest).to_string_lossy().into_owned()
    })
}
