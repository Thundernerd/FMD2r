//! The on-disk cover cache: one body file and one metadata file per entry, evicted least
//! recently used first once the cache outgrows its size cap.

use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A cached cover (or thumbnail).
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) meta: Meta,
    pub(crate) body: Vec<u8>,
}

/// What is stored next to a cached body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Meta {
    pub(crate) content_type: String,
    /// The ETag the browser sees: derived from the body, so it changes only when the image does.
    pub(crate) etag: String,
    /// The upstream validators, sent back when revalidating.
    pub(crate) upstream_etag: Option<String>,
    pub(crate) upstream_last_modified: Option<String>,
    /// When upstream last confirmed the body, in seconds since the Unix epoch.
    pub(crate) fetched_at: u64,
    /// For a thumbnail: the ETag of the cover it was made from.
    pub(crate) source_etag: Option<String>,
}

pub(crate) struct DiskCache {
    dir: PathBuf,
    max_bytes: u64,
}

const BODY: &str = "bin";
const META: &str = "json";

impl DiskCache {
    pub(crate) fn new(dir: PathBuf, max_bytes: u64) -> Self {
        Self { dir, max_bytes }
    }

    fn path(&self, key: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{key}.{ext}"))
    }

    /// The entry stored under `key`, marking it as recently used; `None` when there is none (or
    /// it is unreadable, which the next store repairs).
    pub(crate) fn load(&self, key: &str) -> Option<Entry> {
        let meta = fs::read(self.path(key, META)).ok()?;
        let meta: Meta = serde_json::from_slice(&meta).ok()?;
        let body_path = self.path(key, BODY);
        let body = fs::read(&body_path).ok()?;
        touch(&body_path);
        Some(Entry { meta, body })
    }

    /// Stores `entry` under `key`, then evicts least recently used entries past the size cap.
    pub(crate) fn store(&self, key: &str, entry: &Entry) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let meta = serde_json::to_vec(&entry.meta).map_err(io::Error::other)?;
        // The body goes first: a reader that finds the new metadata finds a matching body.
        write_atomic(&self.path(key, BODY), &entry.body)?;
        write_atomic(&self.path(key, META), &meta)?;
        self.evict()
    }

    /// Rewrites the metadata of `key` only (after upstream answered 304).
    pub(crate) fn store_meta(&self, key: &str, meta: &Meta) -> io::Result<()> {
        let meta = serde_json::to_vec(meta).map_err(io::Error::other)?;
        write_atomic(&self.path(key, META), &meta)
    }

    /// Removes the least recently used entries until the cache fits its cap.
    fn evict(&self) -> io::Result<()> {
        let mut entries = Vec::new();
        let mut total = 0u64;
        for dirent in fs::read_dir(&self.dir)? {
            let dirent = dirent?;
            let path = dirent.path();
            let Ok(md) = dirent.metadata() else { continue };
            total += md.len();
            if path.extension().is_some_and(|e| e == BODY) {
                let meta_len = fs::metadata(path.with_extension(META)).map_or(0, |m| m.len());
                let used = md.modified().unwrap_or(UNIX_EPOCH);
                entries.push((used, md.len() + meta_len, path));
            }
        }
        if total <= self.max_bytes {
            return Ok(());
        }
        entries.sort_by_key(|(used, ..)| *used);
        for (_, len, path) in entries {
            if total <= self.max_bytes {
                break;
            }
            remove(&path.with_extension(META))?;
            remove(&path)?;
            total = total.saturating_sub(len);
        }
        Ok(())
    }
}

/// Seconds since the Unix epoch.
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Writes through a temporary file and a rename, so readers never see half a file.
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Marks a body as just used; its modification time orders eviction.
fn touch(path: &Path) {
    // Best effort: a failed touch only makes the entry look older than it is.
    if let Ok(file) = File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}
