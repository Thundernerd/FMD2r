//! `metadata.db`: the compact local copy of MangaBaka's database that list titles are matched
//! against. It is built in one go into a fresh file ([`MetadataBuilder`]) and only read after
//! that ([`MetadataDb`]); a newer build replaces the whole file.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::error::Result;

const SCHEMA: &str = include_str!("migrations/metadata_v1.sql");

/// The schema version a build writes (`PRAGMA user_version`); a file with another is not read.
const SCHEMA_VERSION: u32 = 1;

/// A MangaBaka series, as kept for matching and display. A title kept only for matching is in
/// the title index, not here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataSeries {
    /// MangaBaka's series ID.
    pub id: i64,
    /// The main title.
    pub title: String,
    /// MangaBaka's `type`: `manga`, `manhwa`, `manhua`, `oel`, `novel`, `other`, ...
    pub kind: String,
    /// MangaBaka's `status`: `releasing`, `completed`, `hiatus`, `cancelled`, `unknown`, ...
    pub status: String,
    pub year: Option<i64>,
    /// `safe`, `suggestive`, `erotica`, `pornographic`, ...
    pub content_rating: String,
    pub description: String,
    pub genres: Vec<String>,
    /// The authors' and artists' names, normalised for matching.
    pub people: Vec<String>,
    /// Cover thumbnails 150, 250 and 350 px tall.
    pub cover_x150: Option<String>,
    pub cover_x250: Option<String>,
    pub cover_x350: Option<String>,
}

/// Writes a new `metadata.db`. Everything goes into one transaction, committed by
/// [`MetadataBuilder::finish`]; dropping the builder before that leaves an empty file.
pub struct MetadataBuilder {
    conn: Connection,
}

impl MetadataBuilder {
    /// Creates the schema in the empty file at `path`.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())?;
        // A file built once and swapped in whole needs no journal: a failed build is thrown away.
        conn.pragma_update(None, "journal_mode", "OFF")?;
        conn.pragma_update(None, "synchronous", "OFF")?;
        conn.execute_batch("BEGIN")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Adds an active series.
    pub fn add_series(&mut self, s: &MetadataSeries) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO series (id, title, type, status, year, content_rating,
                 description, genres, people, cover_x150, cover_x250, cover_x350)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        stmt.execute(params![
            s.id,
            s.title,
            s.kind,
            s.status,
            s.year,
            s.content_rating,
            s.description,
            s.genres.join("\n"),
            s.people.join("\n"),
            s.cover_x150,
            s.cover_x250,
            s.cover_x350,
        ])?;
        Ok(())
    }

    /// Indexes the normalised title `key` for `series`.
    pub fn add_title(&mut self, key: &str, series: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR IGNORE INTO titles (key, series) VALUES (?1, ?2)")?;
        stmt.execute(params![key, series])?;
        Ok(())
    }

    /// Indexes a site link `key` (e.g. `webtoons:4956`) for `series`.
    pub fn add_link(&mut self, key: &str, series: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR IGNORE INTO links (key, series) VALUES (?1, ?2)")?;
        stmt.execute(params![key, series])?;
        Ok(())
    }

    /// Indexes `series`' ID `xid` on `site` (`anilist`, `manga_updates`, ...).
    pub fn add_xid(&mut self, site: &str, xid: &str, series: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR IGNORE INTO xids (site, xid, series) VALUES (?1, ?2, ?3)")?;
        stmt.execute(params![site, xid, series])?;
        Ok(())
    }

    /// Records that `series` was merged into `into`.
    pub fn add_merge(&mut self, series: i64, into: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR REPLACE INTO merged (src, dst) VALUES (?1, ?2)")?;
        stmt.execute(params![series, into])?;
        Ok(())
    }

    /// Points the titles, links and IDs of merged series at the series they were finally merged
    /// into, drops what points at no active series, stamps the build time (Unix milliseconds)
    /// and `build_id` (unique to this build), and commits.
    pub fn finish(self, built_at: i64, build_id: &str) -> Result<()> {
        self.conn.execute_batch(
            "-- Follow chains of merges to their end; a cycle stops where it repeats.
             CREATE TEMP TABLE final AS
             WITH RECURSIVE chain (src, dst, depth) AS (
                 SELECT src, dst, 1 FROM merged
                 UNION ALL
                 SELECT c.src, m.dst, c.depth + 1 FROM chain c JOIN merged m ON m.src = c.dst
                 WHERE c.depth < 32
             )
             SELECT src, dst FROM chain c
             WHERE depth = (SELECT MAX(depth) FROM chain WHERE src = c.src);
             UPDATE OR IGNORE titles SET series = f.dst FROM final f WHERE titles.series = f.src;
             UPDATE OR IGNORE links SET series = f.dst FROM final f WHERE links.series = f.src;
             UPDATE OR IGNORE xids SET series = f.dst FROM final f WHERE xids.series = f.src;
             DELETE FROM titles WHERE series NOT IN (SELECT id FROM series);
             DELETE FROM links WHERE series NOT IN (SELECT id FROM series);
             DELETE FROM xids WHERE series NOT IN (SELECT id FROM series);
             DROP TABLE final;
             DELETE FROM merged;",
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('built_at', ?1), ('build_id', ?2)",
            [built_at.to_string(), build_id.to_owned()],
        )?;
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        self.conn.execute_batch("COMMIT")?;
        self.conn.execute_batch("VACUUM")?;
        Ok(())
    }
}

/// A built `metadata.db`, opened read-only. Cheap to share behind an `Arc`.
pub struct MetadataDb {
    conn: Mutex<Connection>,
}

impl MetadataDb {
    /// Opens the database built at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path.as_ref(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(crate::StoreError::SchemaMismatch {
                db: "metadata.db",
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A read-only connection has no state a panic could leave half-changed.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// When the database was built, in Unix milliseconds.
    pub fn built_at(&self) -> Result<Option<i64>> {
        Ok(self.meta("built_at")?.and_then(|v| v.parse().ok()))
    }

    /// What tells this build from every other.
    pub fn build_id(&self) -> Result<String> {
        Ok(self.meta("build_id")?.unwrap_or_default())
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    /// The active series `id`; `None` for a merged, deleted or unknown one.
    pub fn series(&self, id: i64) -> Result<Option<MetadataSeries>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT id, title, type, status, year, content_rating, description, genres, people,
                    cover_x150, cover_x250, cover_x350
             FROM series WHERE id = ?1",
        )?;
        let split = |s: String| -> Vec<String> {
            s.split('\n')
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .collect()
        };
        Ok(stmt
            .query_row([id], |r| {
                Ok(MetadataSeries {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    kind: r.get(2)?,
                    status: r.get(3)?,
                    year: r.get(4)?,
                    content_rating: r.get(5)?,
                    description: r.get(6)?,
                    genres: split(r.get(7)?),
                    people: split(r.get(8)?),
                    cover_x150: r.get(9)?,
                    cover_x250: r.get(10)?,
                    cover_x350: r.get(11)?,
                })
            })
            .optional()?)
    }

    /// The series one of whose titles normalises to `key`, by ID.
    pub fn by_title_key(&self, key: &str) -> Result<Vec<i64>> {
        self.ids(
            "SELECT DISTINCT series FROM titles WHERE key = ?1 ORDER BY series",
            &[key],
        )
    }

    /// The series with the site link `key`, by ID.
    pub fn by_link_key(&self, key: &str) -> Result<Vec<i64>> {
        self.ids(
            "SELECT DISTINCT series FROM links WHERE key = ?1 ORDER BY series",
            &[key],
        )
    }

    /// The series with ID `xid` on `site`, by ID.
    pub fn by_xid(&self, site: &str, xid: &str) -> Result<Vec<i64>> {
        self.ids(
            "SELECT DISTINCT series FROM xids WHERE site = ?1 AND xid = ?2 ORDER BY series",
            &[site, xid],
        )
    }

    fn ids(&self, sql: &str, args: &[&str]) -> Result<Vec<i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(sql)?;
        let ids = stmt
            .query_map(rusqlite::params_from_iter(args), |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }
}
