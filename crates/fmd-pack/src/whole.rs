//! Writing a file so that it appears under its name only once it is whole.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// The name a file is written under until it is whole: `path` plus `.part`.
pub fn part_path(path: &Path) -> PathBuf {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    part.into()
}

/// Writes `path` through `write`, which is given [`part_path`] to write to; once it returned
/// the file is synced to disk and renamed to `path`, so a process killed or a machine losing
/// power mid-write leaves no partial file under `path`. FMD2 writes its pages and archives in
/// place (docs/tickets/T44-download-hard-crash-resume.md). A failed write removes the part
/// file; one left by a killed process is overwritten by the next write of `path`.
pub fn write_whole<E: From<io::Error>>(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), E>,
) -> Result<(), E> {
    let part = part_path(path);
    let written = write(&part).and_then(|()| {
        File::open(&part)?.sync_all()?;
        fs::rename(&part, path)?;
        Ok(())
    });
    if written.is_err() {
        let _ = fs::remove_file(&part);
    }
    written
}
