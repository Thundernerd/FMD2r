//! The local copy of MangaBaka's database (`metadata.db`).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use fmd_http::TerminateToken;
use fmd_store::{MetadataBuilder, MetadataDb, MetadataSeries, StoreError};

use super::build::{BuildProgress, BuildSummary, Counting, read_dump};
use super::normalize::{link_key, title_keys};
use super::{DumpSource, MetadataError};

pub const DUMP_URL: &str = "https://api.mangabaka.org/v1/database/series.jsonl.zst";
/// In the data dir.
pub const METADATA_DB: &str = "metadata.db";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbInfo {
    /// Unix milliseconds.
    pub built_at: i64,
    pub bytes: u64,
}

/// A built `metadata.db`; lookups normalise their input as the build did.
pub struct Metadata {
    db: MetadataDb,
}

impl Metadata {
    /// `None` for a merged, deleted or unknown series.
    pub fn series(&self, id: i64) -> Result<Option<MetadataSeries>, StoreError> {
        self.db.series(id)
    }

    /// Matches with or without the title's decorations.
    pub fn find_by_title(&self, title: &str) -> Result<Vec<i64>, StoreError> {
        let mut ids = Vec::new();
        for key in title_keys(title) {
            ids.extend(self.db.by_title_key(&key)?);
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    /// `site` is a MangaBaka `source` key such as `anilist` or `manga_updates`.
    pub fn find_by_xid(&self, site: &str, xid: &str) -> Result<Vec<i64>, StoreError> {
        self.db.by_xid(site, xid)
    }

    /// Only for sites with stable IDs (WebToons).
    pub fn find_by_link(&self, url: &str) -> Result<Vec<i64>, StoreError> {
        match link_key(url) {
            Some(key) => self.db.by_link_key(&key),
            None => Ok(Vec::new()),
        }
    }

    /// Unique per build.
    pub(super) fn build_id(&self) -> Result<String, StoreError> {
        self.db.build_id()
    }

    pub(super) fn by_title_key(&self, key: &str) -> Result<Vec<i64>, StoreError> {
        self.db.by_title_key(key)
    }

    pub(super) fn by_link_key(&self, key: &str) -> Result<Vec<i64>, StoreError> {
        self.db.by_link_key(key)
    }
}

/// `metadata.db` in a data dir: absent until the first download.
pub struct MangaBakaDb {
    dir: PathBuf,
    source: Arc<dyn DumpSource>,
    current: RwLock<Option<Arc<Metadata>>>,
    /// Held while a build or removal changes the file.
    changing: Mutex<()>,
}

impl MangaBakaDb {
    /// A file that cannot be read counts as none.
    pub fn open(dir: impl AsRef<Path>, source: Arc<dyn DumpSource>) -> Self {
        let dir = dir.as_ref().to_owned();
        let path = dir.join(METADATA_DB);
        let current = if path.exists() {
            match MetadataDb::open(&path) {
                Ok(db) => Some(Arc::new(Metadata { db })),
                Err(e) => {
                    tracing::warn!(target: "fmd_core", "{}: {e}", path.display());
                    None
                }
            }
        } else {
            None
        };
        Self {
            dir,
            source,
            current: RwLock::new(current),
            changing: Mutex::new(()),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(METADATA_DB)
    }

    pub fn current(&self) -> Option<Arc<Metadata>> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn info(&self) -> Option<DbInfo> {
        let built_at = self.current()?.db.built_at().ok().flatten()?;
        let bytes = std::fs::metadata(self.path()).ok()?.len();
        Some(DbInfo { built_at, bytes })
    }

    /// Builds a new database from [`DUMP_URL`] and swaps it in with one rename; on failure the
    /// old one stays. Blocking.
    pub fn refresh(
        &self,
        terminate: &TerminateToken,
        progress: &mut dyn FnMut(&BuildProgress),
    ) -> Result<BuildSummary, MetadataError> {
        let _changing = self.changing.lock().unwrap_or_else(PoisonError::into_inner);
        std::fs::create_dir_all(&self.dir)?;
        let download = self.source.open(DUMP_URL, terminate)?;
        let bytes = Arc::new(AtomicU64::new(0));
        let counting = Counting {
            inner: download.reader,
            bytes: bytes.clone(),
            terminate: terminate.clone(),
        };
        let jsonl = zstd::stream::read::Decoder::new(counting).map_err(MetadataError::Read)?;

        let part = tempfile::Builder::new()
            .prefix(".metadata.db.")
            .suffix(".part")
            .tempfile_in(&self.dir)?;
        let mut builder = MetadataBuilder::create(part.path())?;
        let total_bytes = download.length;
        let summary = read_dump(jsonl, &mut builder, terminate, |series| {
            progress(&BuildProgress {
                bytes: bytes.load(std::sync::atomic::Ordering::Relaxed),
                total_bytes,
                series,
            });
        })?;
        let built_at = now_ms();
        let build_id = format!("{built_at}-{}", std::process::id());
        builder.finish(built_at, &build_id)?;

        let path = self.path();
        part.persist(&path)
            .map_err(|e| MetadataError::Io(e.error))?;
        let db = MetadataDb::open(&path)?;
        *self.current.write().unwrap_or_else(PoisonError::into_inner) =
            Some(Arc::new(Metadata { db }));
        Ok(summary)
    }

    /// Lookups already holding the database keep reading it until they let go.
    pub fn remove(&self) -> Result<(), MetadataError> {
        let _changing = self.changing.lock().unwrap_or_else(PoisonError::into_inner);
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = None;
        match std::fs::remove_file(self.path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
