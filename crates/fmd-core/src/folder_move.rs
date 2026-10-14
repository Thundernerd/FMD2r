//! Moving a library series' downloaded chapters along when its download folder changes, as
//! FMD2 offers when a favorite's folder changes (`RS_DlgMoveSaveToFiles`,
//! mangadownloader/forms/frmMain.pas:963-964).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// What moving one folder did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MoveReport {
    /// Files and folders moved out of the old folder.
    pub moved: usize,
    /// Files and folders left in the old folder because one of that name is in the new one.
    pub skipped: Vec<PathBuf>,
    /// What could not be moved, and why.
    pub failures: Vec<(PathBuf, String)>,
}

/// The folders of the rest of the library, which a series' move must leave alone.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LibraryFolders {
    /// The download folders of the other library series.
    pub series: Vec<PathBuf>,
    /// The destinations' paths and the websites' own download folders: with "generate manga
    /// folder" off, a series downloads straight into one of them.
    pub destinations: Vec<PathBuf>,
}

impl LibraryFolders {
    /// Why moving all of `folder` would take files that are not its series' along.
    pub fn sharing(&self, folder: &Path) -> Option<SharedFolder> {
        let within = |other: &Path| other.starts_with(folder);
        if let Some(d) = self.destinations.iter().find(|d| within(d)) {
            return Some(SharedFolder::Destination(d.clone()));
        }
        self.series
            .iter()
            .find(|s| within(s))
            .map(|s| SharedFolder::Series(s.clone()))
    }
}

/// A series folder that holds more than that series' downloads.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SharedFolder {
    /// It is, or holds, another series' folder.
    #[error("another series downloads to {}", .0.display())]
    Series(PathBuf),
    /// It is, or holds, a destination's folder.
    #[error("it holds the destination {}", .0.display())]
    Destination(PathBuf),
}

/// Renames a file or folder, as [`fs::rename`] does.
type Rename = dyn Fn(&Path, &Path) -> io::Result<()> + Send + Sync;

/// Moves the contents of a folder into another.
pub struct FolderMover {
    rename: Box<Rename>,
}

impl Default for FolderMover {
    fn default() -> Self {
        Self {
            rename: Box::new(|from, to| fs::rename(from, to)),
        }
    }
}

impl FolderMover {
    /// Renames with `rename` instead of [`fs::rename`], e.g. one that fails as a rename across
    /// file systems does.
    pub fn with_rename(
        rename: impl Fn(&Path, &Path) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            rename: Box::new(rename),
        }
    }

    /// [`FolderMover::move_folder`] for a library series, unless its folder is shared with the
    /// rest of `library`: then nothing moves.
    pub fn move_series(
        &self,
        from: &Path,
        to: &Path,
        library: &LibraryFolders,
    ) -> Result<MoveReport, SharedFolder> {
        match library.sharing(from) {
            Some(shared) => Err(shared),
            None => Ok(self.move_folder(from, to)),
        }
    }

    /// Moves what is in `from` into `to`, then removes `from` once empty. A `from` that does
    /// not exist, or is `to`, is nothing to move.
    pub fn move_folder(&self, from: &Path, to: &Path) -> MoveReport {
        let mut report = MoveReport::default();
        if !from.is_dir() || same_folder(from, to) {
            return report;
        }
        if let Err(e) = self.move_entries(from, to, &mut report) {
            report.failures.push((from.to_owned(), e.to_string()));
        }
        report
    }

    fn move_entries(&self, from: &Path, to: &Path, report: &mut MoveReport) -> io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let source = entry?.path();
            let Some(name) = source.file_name() else {
                continue;
            };
            if to.starts_with(&source) {
                // The new folder is inside the old one: it stays where it is.
                continue;
            }
            let target = to.join(name);
            if fs::symlink_metadata(&target).is_ok() {
                report.skipped.push(source);
                continue;
            }
            match self.move_entry(&source, &target) {
                Ok(()) => report.moved += 1,
                Err(e) => report.failures.push((source, e.to_string())),
            }
        }
        if fs::read_dir(from)?.next().is_none() {
            fs::remove_dir(from)?;
        }
        Ok(())
    }

    /// A rename, else (across file systems, where a rename fails) a copy and then a delete of
    /// the source. A failed copy is undone, so the source stays whole.
    fn move_entry(&self, source: &Path, target: &Path) -> io::Result<()> {
        if (self.rename)(source, target).is_ok() {
            return Ok(());
        }
        if let Err(e) = copy(source, target) {
            // `target` did not exist before, so all of it is the partial copy.
            let _ = remove(target);
            return Err(e);
        }
        remove(source)
    }
}

/// Compares the real paths when both exist, else the paths as given.
fn same_folder(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.components().eq(b.components()),
    }
}

fn copy(source: &Path, target: &Path) -> io::Result<()> {
    if fs::metadata(source)?.is_dir() {
        fs::create_dir(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy(&entry.path(), &target.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(source, target).map(drop)
    }
}

fn remove(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Whether one folder is the other or holds it, comparing the paths as given.
pub fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// The folders a move is working in, shared with the download engine so no task downloads into
/// one meanwhile: never two writers in one folder. Cheap to clone.
#[derive(Debug, Default, Clone)]
pub struct MovingFolders(Arc<Mutex<Vec<PathBuf>>>);

impl MovingFolders {
    /// Holds `folders` until the guard is dropped.
    pub fn hold(&self, folders: Vec<PathBuf>) -> MovingGuard {
        self.lock().extend(folders.iter().cloned());
        MovingGuard {
            moving: self.clone(),
            folders,
        }
    }

    /// Whether a task downloading to `dir` has to wait for a move.
    pub fn blocks(&self, dir: &Path) -> bool {
        self.lock().iter().any(|f| overlap(f, dir))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<PathBuf>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Releases its folders when dropped.
#[derive(Debug)]
pub struct MovingGuard {
    moving: MovingFolders,
    folders: Vec<PathBuf>,
}

impl Drop for MovingGuard {
    fn drop(&mut self) {
        let mut held = self.moving.lock();
        for folder in &self.folders {
            if let Some(i) = held.iter().position(|f| f == folder) {
                held.swap_remove(i);
            }
        }
    }
}
