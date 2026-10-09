//! Log lines on disk: JSON lines in `<data dir>/logs/`, rotated by size and count. FMD2 appends
//! to one unbounded file through MultiLog (baseunits/uBaseUnit.pas); FMD2r bounds the disk use.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fmd_core::settings::LogSettings;

use crate::logs::LogLine;

/// The file being written; older ones are `fmd2r.1.log` (newest) to `fmd2r.<max_files - 1>.log`.
const CURRENT: &str = "fmd2r.log";

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

    fn max_files(&self) -> usize {
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
}

impl LogWriter {
    /// Opens (creating) `dir` and its current file, removing files beyond `rotation.max_files`
    /// left over from a larger setting.
    pub fn open(dir: &Path, rotation: LogRotation) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        for (index, path) in numbered_files(dir)? {
            if index >= rotation.max_files() {
                fs::remove_file(path)?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(dir.join(CURRENT))?;
        let mut size = file.metadata()?.len();
        // A crash can leave a partial line; end it so the next line parses.
        if size > 0 {
            let mut last = [0u8];
            file.seek(SeekFrom::End(-1))?;
            file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                file.write_all(b"\n")?;
                size += 1;
            }
        }
        Ok(Self {
            dir: dir.to_owned(),
            rotation,
            file: Some(file),
            size,
        })
    }

    /// The directory written to.
    pub fn dir(&self) -> &Path {
        &self.dir
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
            None => self.file.insert(append(&self.dir.join(CURRENT))?),
        };
        file.write_all(&bytes)?;
        self.size = self.size.saturating_add(len);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        // Closed first: an open file can't be renamed on Windows.
        self.file = None;
        let current = self.dir.join(CURRENT);
        let max = self.rotation.max_files();
        if max == 1 {
            fs::remove_file(&current)?;
        }
        for index in (1..max).rev() {
            let from = if index == 1 {
                current.clone()
            } else {
                self.dir.join(numbered(index - 1))
            };
            if from.exists() {
                fs::rename(from, self.dir.join(numbered(index)))?;
            }
        }
        self.size = 0;
        self.file = Some(append(&current)?);
        Ok(())
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// The log files in `dir`, oldest first.
pub(crate) fn files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut numbered = numbered_files(dir)?;
    numbered.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
    let mut paths: Vec<PathBuf> = numbered.into_iter().map(|(_, path)| path).collect();
    let current = dir.join(CURRENT);
    if current.exists() {
        paths.push(current);
    }
    Ok(paths)
}

/// Every line in the log files in `dir`, oldest first, passed to `each`. Lines that don't parse
/// (a partial line left by a crash) are skipped.
pub(crate) fn read_lines(dir: &Path, mut each: impl FnMut(LogLine)) -> io::Result<()> {
    for path in files(dir)? {
        for line in BufReader::new(File::open(path)?).lines() {
            if let Ok(line) = serde_json::from_str(&line?) {
                each(line);
            }
        }
    }
    Ok(())
}

fn numbered(index: usize) -> String {
    format!("fmd2r.{index}.log")
}

/// The rotated files in `dir` with their index.
fn numbered_files(dir: &Path) -> io::Result<Vec<(usize, PathBuf)>> {
    let mut found = Vec::new();
    if !dir.exists() {
        return Ok(found);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let index = name
            .to_str()
            .and_then(|n| n.strip_prefix("fmd2r."))
            .and_then(|n| n.strip_suffix(".log"))
            .and_then(|n| n.parse::<usize>().ok());
        if let Some(index) = index {
            found.push((index, entry.path()));
        }
    }
    Ok(found)
}
