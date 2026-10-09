//! The local copy of MangaBaka's database: downloading and building `metadata.db`, swapping it
//! in, removing it, and looking series up in it.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use fmd_http::TerminateToken;
use fmd_store::{MetadataBuilder, MetadataDb, MetadataSeries, StoreError};

use super::build::{BuildProgress, BuildSummary, Counting, read_dump};
use super::normalize::{link_key, title_keys};
use super::{DumpSource, MetadataError};

/// Where MangaBaka publishes its nightly dump.
pub const DUMP_URL: &str = "https://api.mangabaka.org/v1/database/series.jsonl.zst";
/// The database's file name in the data dir.
pub const METADATA_DB: &str = "metadata.db";

/// The database file at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbInfo {
    /// When it was built, in Unix milliseconds.
    pub built_at: i64,
    /// Its size on disk.
    pub bytes: u64,
}

/// A built `metadata.db`, with lookups that normalise what they are given the way the build
/// normalised MangaBaka's titles and links.
pub struct Metadata {
    db: MetadataDb,
}

impl Metadata {
    /// The active series `id`; `None` for a merged, deleted or unknown one.
    pub fn series(&self, id: i64) -> Result<Option<MetadataSeries>, StoreError> {
        self.db.series(id)
    }

    /// The series with a title that matches `title`, with or without its decorations.
    pub fn find_by_title(&self, title: &str) -> Result<Vec<i64>, StoreError> {
        let mut ids = Vec::new();
        for key in title_keys(title) {
            ids.extend(self.db.by_title_key(&key)?);
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    /// The series with ID `xid` on `site` (MangaBaka's `source` keys: `anilist`,
    /// `manga_updates`, `my_anime_list`, `kitsu`, `anime_planet`, ...).
    pub fn find_by_xid(&self, site: &str, xid: &str) -> Result<Vec<i64>, StoreError> {
        self.db.by_xid(site, xid)
    }

    /// The series that link to the page at `url`, for sites with stable IDs (WebToons).
    pub fn find_by_link(&self, url: &str) -> Result<Vec<i64>, StoreError> {
        match link_key(url) {
            Some(key) => self.db.by_link_key(&key),
            None => Ok(Vec::new()),
        }
    }

    /// What tells this build of the database from every other.
    pub(super) fn build_id(&self) -> Result<String, StoreError> {
        self.db.build_id()
    }

    /// The series with a title that normalises to `key`.
    pub(super) fn by_title_key(&self, key: &str) -> Result<Vec<i64>, StoreError> {
        self.db.by_title_key(key)
    }

    /// The series with the site link `key`.
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
    /// The database in `dir`, downloaded from `source`. A file that cannot be read counts as
    /// none.
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

    /// The database, when one is downloaded.
    pub fn current(&self) -> Option<Arc<Metadata>> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// When the database was built and how large it is; `None` when there is none.
    pub fn info(&self) -> Option<DbInfo> {
        let built_at = self.current()?.db.built_at().ok().flatten()?;
        let bytes = std::fs::metadata(self.path()).ok()?.len();
        Some(DbInfo { built_at, bytes })
    }

    /// Downloads the dump from [`DUMP_URL`] and builds a new database from it, streaming, then
    /// swaps it in for the old one in one rename. A build that fails or is cancelled leaves the
    /// old database as it was. Blocks, so call it from a thread outside any tokio runtime.
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

    /// Deletes the database. Lookups already holding it keep reading it until they let go.
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

/// Now, in Unix milliseconds.
pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
