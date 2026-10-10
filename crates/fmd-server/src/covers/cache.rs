//! The on-disk cover cache: one body file and one metadata file per entry, evicted least
//! recently used first once the cache outgrows its size cap.

use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::fetch::Validators;

/// A cached cover (or thumbnail).
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) meta: Meta,
    pub(crate) body: Vec<u8>,
}

impl Entry {
    /// An entry for `body`, with an ETag derived from it.
    pub(crate) fn new(
        body: Vec<u8>,
        content_type: String,
        upstream: Validators,
        fetched_at: u64,
        source_etag: Option<String>,
    ) -> Self {
        let digest = Sha256::digest(&body);
        let etag: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        Self {
            meta: Meta {
                content_type,
                etag: format!("\"{etag}\""),
                upstream,
                fetched_at,
                source_etag,
            },
            body,
        }
    }
}

/// What is stored next to a cached body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Meta {
    pub(crate) content_type: String,
    /// The ETag the browser sees: derived from the body, so it changes only when the image does.
    pub(crate) etag: String,
    /// The upstream validators, sent back when revalidating.
    pub(crate) upstream: Validators,
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

    /// Marks the entry as recently used; an unreadable one is `None` until the next store.
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

    /// LRU eviction, after clearing leftovers of interrupted writes (temp files, half entries).
    fn evict(&self) -> io::Result<()> {
        let mut entries = Vec::new();
        let mut total = 0u64;
        for dirent in fs::read_dir(&self.dir)? {
            let dirent = dirent?;
            let path = dirent.path();
            let Ok(md) = dirent.metadata() else { continue };
            let modified = md.modified().unwrap_or(UNIX_EPOCH);
            let age = SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            let leftover = match ext {
                "tmp" => age > LEFTOVER_AGE,
                META => !path.with_extension(BODY).exists(),
                // A body without metadata is either being stored right now or left over.
                BODY => !path.with_extension(META).exists() && age > LEFTOVER_AGE,
                _ => false,
            };
            if leftover {
                remove(&path)?;
                continue;
            }
            total += md.len();
            if ext == BODY {
                let meta_len = fs::metadata(path.with_extension(META)).map_or(0, |m| m.len());
                entries.push((modified, md.len() + meta_len, path));
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

/// How old a temporary file or half an entry must be to count as left over by an interrupted
/// write rather than one in progress.
const LEFTOVER_AGE: Duration = Duration::from_secs(60 * 60);

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
