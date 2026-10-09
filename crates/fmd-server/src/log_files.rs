//! Log lines on disk: JSON lines in `<data dir>/logs/`, rotated by size and count. FMD2 appends
//! to one unbounded file through MultiLog (baseunits/uBaseUnit.pas); FMD2r bounds the disk use.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fmd_core::settings::LogSettings;

use crate::logs::LogLine;

/// Index of the file being written, `fmd2r.log`; older ones are `fmd2r.1.log` (newest) to
/// `fmd2r.<max_files - 1>.log` (see [`file_name`]).
const CURRENT: usize = 0;

/// How big the log files grow and how many are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRotation {
    /// A file is rotated before a line would take it past this size (a single longer line still
    /// gets a file of its own).
    pub max_file_bytes: u64,
    /// Files kept, the one being written included. Minimum 1.
    pub max_files: usize,
}

impl Default for LogRotation {
    /// 10 MiB × 5.
    fn default() -> Self {
        Self::from_settings(&LogSettings::default())
    }
}

impl LogRotation {
    pub fn from_settings(settings: &LogSettings) -> Self {
        Self {
            max_file_bytes: u64::from(settings.max_file_size_mb) * 1024 * 1024,
            max_files: usize::try_from(settings.max_files).unwrap_or(usize::MAX),
        }
    }

    /// `max_files`, at least 1: there is always a file being written.
    fn kept_files(&self) -> usize {
        self.max_files.max(1)
    }
}

/// Appends log lines to the files in a directory, one JSON object per line, rotating them as
/// [`LogRotation`] says.
pub struct LogWriter {
    dir: PathBuf,
    rotation: LogRotation,
    /// `None` only while rotating, or after a rotation failed to reopen the file.
    file: Option<File>,
    size: u64,
    /// `open` found the current file ending in a partial line (a crash mid-write).
    pub(crate) repaired: bool,
}

impl LogWriter {
    /// Opens (creating) `dir` and its current file, removing files beyond `rotation.max_files`
    /// left over from a larger setting.
    pub fn open(dir: &Path, rotation: LogRotation) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        for (index, path) in indexed_files(dir)? {
            if index >= rotation.kept_files() {
                fs::remove_file(path)?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(dir.join(file_name(CURRENT)))?;
        let mut size = file.metadata()?.len();
        let mut repaired = false;
        // A crash can leave a partial line; end it so the next line parses.
        if size > 0 {
            let mut last = [0u8];
            file.seek(SeekFrom::End(-1))?;
            file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                file.write_all(b"\n")?;
                size += 1;
                repaired = true;
            }
        }
        Ok(Self {
            dir: dir.to_owned(),
            rotation,
            file: Some(file),
            size,
            repaired,
        })
    }

    /// Every log file, oldest first, opened with its current length, so it can be read without
    /// holding up writes: an open file stays readable when rotation renames or removes it.
    pub(crate) fn snapshot(&self) -> io::Result<Vec<(File, u64)>> {
        let mut files = Vec::new();
        for path in files_oldest_first(&self.dir)? {
            let file = File::open(path)?;
            let len = file.metadata()?.len();
            files.push((file, len));
        }
        Ok(files)
    }

    /// Appends `line`, rotating first when it would take the current file past the size limit.
    pub fn write(&mut self, line: &LogLine) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(line).map_err(io::Error::other)?;
        bytes.push(b'\n');
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if self.size > 0 && self.size.saturating_add(len) > self.rotation.max_file_bytes {
            self.rotate()?;
        }
        let file = match &mut self.file {
            Some(file) => file,
            None => self
                .file
                .insert(append(&self.dir.join(file_name(CURRENT)))?),
        };
        file.write_all(&bytes)?;
        self.size = self.size.saturating_add(len);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        // Closed first: an open file can't be renamed on Windows.
        self.file = None;
        let kept = self.rotation.kept_files();
        if kept == 1 {
            fs::remove_file(self.dir.join(file_name(CURRENT)))?;
        }
        // Renaming onto the oldest kept index replaces (removes) the file there.
        for index in (1..kept).rev() {
            let from = self.dir.join(file_name(index - 1));
            if from.exists() {
                fs::rename(from, self.dir.join(file_name(index)))?;
            }
        }
        self.size = 0;
        self.file = Some(append(&self.dir.join(file_name(CURRENT)))?);
        Ok(())
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// The log files in `dir`, oldest first.
fn files_oldest_first(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = indexed_files(dir)?;
    files.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// The newest `count` (or more) lines in the log files in `dir`, oldest first, read from the
/// newest file back, and the highest sequence number among them. Lines that don't parse (a
/// partial line left by a crash) are skipped.
pub(crate) fn read_tail(dir: &Path, count: usize) -> io::Result<(Vec<LogLine>, u64)> {
    let mut tail: Vec<LogLine> = Vec::new();
    for path in files_oldest_first(dir)?.into_iter().rev() {
        if tail.len() >= count {
            break;
        }
        let mut lines: Vec<LogLine> = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            if let Ok(line) = serde_json::from_str(&line?) {
                lines.push(line);
            }
        }
        lines.append(&mut tail);
        tail = lines;
    }
    let last_seq = tail.iter().map(|l| l.seq).max().unwrap_or(0);
    Ok((tail, last_seq))
}

/// `fmd2r.log` for [`CURRENT`], else `fmd2r.<index>.log`.
fn file_name(index: usize) -> String {
    if index == CURRENT {
        "fmd2r.log".to_owned()
    } else {
        format!("fmd2r.{index}.log")
    }
}

/// The log files in `dir` with their index, the inverse of [`file_name`].
fn indexed_files(dir: &Path) -> io::Result<Vec<(usize, PathBuf)>> {
    let mut found = Vec::new();
    if !dir.exists() {
        return Ok(found);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let index = match name.to_str() {
            Some("fmd2r.log") => Some(CURRENT),
            Some(n) => n
                .strip_prefix("fmd2r.")
                .and_then(|n| n.strip_suffix(".log"))
                .and_then(|n| n.parse().ok())
                .filter(|&index| index != CURRENT),
            None => None,
        };
        if let Some(index) = index {
            found.push((index, entry.path()));
        }
    }
    Ok(found)
}
