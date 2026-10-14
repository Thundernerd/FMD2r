//! Moving library series' downloaded chapters to their new folder in the background (#283), as
//! FMD2 offers when a favorite's folder changes (`RS_DlgMoveSaveToFiles`,
//! mangadownloader/forms/frmMain.pas:963-964).

use std::path::{Path, PathBuf};
use std::time::Duration;

use fmd_core::download::TaskStatus;
use fmd_core::folder_move::{FolderMover, LibraryFolders, MoveReport, SharedFolder, overlap};
use fmd_core::jobs::{JobPhase, JobStatus};
use fmd_store::{EventSeverity, FavoriteId, NewEvent};

use crate::one_off_job::OneOffJob;
use crate::state::off_thread;
use crate::{ApiError, AppState};

/// The `events` kind of a move's inbox item.
const EVENT_KIND: &str = "downloads_moved";

/// How often a move looks whether a series' running downloads have ended.
const TASK_POLL: Duration = Duration::from_millis(250);

/// One series whose downloads go from its old folder to its new one.
#[derive(Debug, Clone)]
pub(crate) struct SeriesMove {
    pub(crate) id: FavoriteId,
    pub(crate) title: String,
    pub(crate) from: String,
    pub(crate) to: String,
}

/// The `move-downloads` job, for the System page; it only runs through a folder change.
pub(crate) fn job() -> OneOffJob {
    OneOffJob::new(
        JOB_ID,
        JOB_TITLE,
        "a move starts with a series' folder change",
        "a move cannot be cancelled",
    )
}

const JOB_ID: &str = "move-downloads";
const JOB_TITLE: &str = "Move downloads";

/// A move reserved before the new folders are saved, so a request that cannot move answers 409
/// without changing anything. Dropped without [`MoveRun::start`], it gives the reservation back.
pub(crate) struct MoveRun {
    state: AppState,
    previous: Option<JobStatus>,
}

impl MoveRun {
    /// A conflict, naming the job, while a move runs.
    pub(crate) fn reserve(state: &AppState) -> Result<Self, ApiError> {
        let previous = state.move_job.begin(&state.jobs).map_err(|_| {
            ApiError::Conflict(format!("{JOB_TITLE} is running; try again once it is done"))
        })?;
        Ok(Self {
            state: state.clone(),
            previous: Some(previous),
        })
    }

    /// Moves the series in the background, then reports in the inbox.
    pub(crate) fn start(mut self, moves: Vec<SeriesMove>) {
        self.previous = None;
        tokio::spawn(run(self.state.clone(), moves));
    }
}

impl Drop for MoveRun {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            self.state.move_job.update(|s| *s = previous);
            self.state.jobs.changed(JOB_ID);
        }
    }
}

async fn run(state: AppState, moves: Vec<SeriesMove>) {
    let total = moves.len() as u64;
    progress(&state, 0, total);
    let mut summary = Summary::default();
    for (done, series) in (1..).zip(&moves) {
        let outcome = move_series(&state, series, &moves).await;
        summary.add(series, outcome);
        progress(&state, done, total);
    }
    state.move_job.update(|s| s.phase = JobPhase::Done);
    state.jobs.changed(JOB_ID);
    if summary.moved > 0 {
        state.request_comic_server_scan();
    }
    if let Err(e) = state.notify(summary.event()).await {
        tracing::warn!(target: "fmd_server", "adding the move's report to the inbox: {e}");
    }
}

fn progress(state: &AppState, done: u64, total: u64) {
    state.move_job.update(|s| {
        s.done = done;
        s.total = total;
    });
    state.jobs.changed(JOB_ID);
}

/// How one series' move went.
enum Outcome {
    Moved(MoveReport),
    /// Its old folder holds more than its own downloads, so nothing moved.
    Shared(SharedFolder),
    Failed(ApiError),
}

/// Waits for the series' running downloads to end and keeps new ones from starting while its
/// files move; its unfinished downloads then follow it to the new folder.
async fn move_series(state: &AppState, series: &SeriesMove, batch: &[SeriesMove]) -> Outcome {
    let (from, to) = (PathBuf::from(&series.from), PathBuf::from(&series.to));
    if from == to {
        return Outcome::Moved(MoveReport::default());
    }
    // A shared folder is refused before waiting on downloads into it.
    match library_folders(state, series, batch).await {
        Ok(library) => {
            if let Some(shared) = library.sharing(&from) {
                return Outcome::Shared(shared);
            }
        }
        Err(e) => return Outcome::Failed(e),
    }
    let held = state.moving.hold(vec![from.clone(), to.clone()]);
    let moved = held_move(state, series, batch, from, to).await;
    drop(held);
    if let Err(e) = state.engine.activate_waiting().await {
        tracing::warn!(target: "fmd_server", "starting downloads held for a move: {e}");
    }
    match moved {
        Ok(Ok(report)) => Outcome::Moved(report),
        Ok(Err(shared)) => Outcome::Shared(shared),
        Err(e) => Outcome::Failed(e),
    }
}

/// The move while its folders are held. The library is read again once the downloads have
/// ended, so a series given the old folder meanwhile is not moved along.
async fn held_move(
    state: &AppState,
    series: &SeriesMove,
    batch: &[SeriesMove],
    from: PathBuf,
    to: PathBuf,
) -> Result<Result<MoveReport, SharedFolder>, ApiError> {
    wait_for_downloads(state, &from, &to).await?;
    let library = library_folders(state, series, batch).await?;
    let report = {
        let (from, to) = (from.clone(), to.clone());
        off_thread(move || FolderMover::default().move_series(&from, &to, &library)).await?
    };
    if report.is_ok() {
        repoint_downloads(state, from, series.to.clone()).await?;
    }
    Ok(report)
}

/// The queue tasks still to download into `from` download into `to` instead; finished ones keep
/// the folder they downloaded to.
async fn repoint_downloads(state: &AppState, from: PathBuf, to: String) -> Result<(), ApiError> {
    state
        .blocking(move |db| -> Result<(), ApiError> {
            for task in db.tasks().list()? {
                if task.status != TaskStatus::Finished && Path::new(&task.save_to) == from {
                    db.tasks().set_save_to(task.id, &to)?;
                }
            }
            Ok(())
        })
        .await
}

async fn wait_for_downloads(state: &AppState, from: &Path, to: &Path) -> Result<(), ApiError> {
    loop {
        let busy = state.engine.list().await?.into_iter().any(|t| {
            let dir = Path::new(&t.task.save_to);
            t.running && (overlap(dir, from) || overlap(dir, to))
        });
        if !busy {
            return Ok(());
        }
        tokio::time::sleep(TASK_POLL).await;
    }
}

/// The rest of the library: every other series' folder, old and new, and the destinations and
/// websites' own folders.
async fn library_folders(
    state: &AppState,
    series: &SeriesMove,
    batch: &[SeriesMove],
) -> Result<LibraryFolders, ApiError> {
    let id = series.id;
    let mut library = LibraryFolders {
        series: batch
            .iter()
            .filter(|other| other.id != id)
            .map(|other| PathBuf::from(&other.from))
            .collect(),
        destinations: state
            .settings
            .get()
            .saveto
            .destinations
            .iter()
            .map(|d| PathBuf::from(&d.path))
            .collect(),
    };
    let (series, websites) = state
        .blocking(move |db| -> Result<_, ApiError> {
            let series: Vec<PathBuf> = db
                .favorites()
                .list()?
                .into_iter()
                .filter(|f| f.id != id)
                .map(|f| PathBuf::from(f.save_to))
                .collect();
            let mut websites = Vec::new();
            for module in db.module_settings().module_ids()? {
                if let Some(stored) = db.module_settings().get(&module)?
                    && !stored.save_to.trim().is_empty()
                {
                    websites.push(PathBuf::from(stored.save_to));
                }
            }
            Ok((series, websites))
        })
        .await?;
    library.series.extend(series);
    library.destinations.extend(websites);
    Ok(library)
}

/// What a run did, for its inbox item.
#[derive(Default)]
struct Summary {
    /// Series with at least one file or folder moved.
    moved: usize,
    /// What was skipped or failed, one line each.
    problems: Vec<String>,
}

impl Summary {
    fn add(&mut self, series: &SeriesMove, outcome: Outcome) {
        let title = &series.title;
        match outcome {
            Outcome::Moved(report) => {
                if report.moved > 0 {
                    self.moved += 1;
                }
                for skipped in report.skipped {
                    self.problems.push(format!(
                        "{title}: skipped {}: the new folder has one of that name",
                        skipped.display()
                    ));
                }
                for (path, error) in report.failures {
                    self.problems
                        .push(format!("{title}: failed {}: {error}", path.display()));
                }
            }
            Outcome::Shared(shared) => self
                .problems
                .push(format!("{title}: skipped the series: {shared}")),
            Outcome::Failed(e) => self.problems.push(format!("{title}: failed: {e}")),
        }
    }

    fn event(self) -> NewEvent {
        NewEvent {
            kind: EVENT_KIND.into(),
            severity: if self.problems.is_empty() {
                EventSeverity::Info
            } else {
                EventSeverity::Warning
            },
            module_id: None,
            task_id: None,
            title: format!("Moved the downloads of {} series", self.moved),
            body: self.problems.join("\n").into(),
        }
    }
}
