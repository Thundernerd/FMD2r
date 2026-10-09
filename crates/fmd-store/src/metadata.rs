//! `metadata.db`: a compact local copy of MangaBaka's database, built once into a fresh file
//! and then only read; a newer build replaces the whole file.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::error::Result;

const SCHEMA: &str = include_str!("migrations/metadata_v1.sql");

/// A file with another `user_version` is not read.
const SCHEMA_VERSION: u32 = 1;

/// A MangaBaka series. Titles kept only for matching live in the title index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataSeries {
    pub id: i64,
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

/// Writes a new `metadata.db` in one transaction committed by [`MetadataBuilder::finish`];
/// dropping the builder first leaves an empty file.
pub struct MetadataBuilder {
    conn: Connection,
}

impl MetadataBuilder {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())?;
        // No journal: a failed build is thrown away.
        conn.pragma_update(None, "journal_mode", "OFF")?;
        conn.pragma_update(None, "synchronous", "OFF")?;
        conn.execute_batch("BEGIN")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

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

    pub fn add_title(&mut self, key: &str, series: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR IGNORE INTO titles (key, series) VALUES (?1, ?2)")?;
        stmt.execute(params![key, series])?;
        Ok(())
    }

    /// `key` is e.g. `webtoons:4956`.
    pub fn add_link(&mut self, key: &str, series: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR IGNORE INTO links (key, series) VALUES (?1, ?2)")?;
        stmt.execute(params![key, series])?;
        Ok(())
    }

    /// `site` is e.g. `anilist` or `manga_updates`.
    pub fn add_xid(&mut self, site: &str, xid: &str, series: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR IGNORE INTO xids (site, xid, series) VALUES (?1, ?2, ?3)")?;
        stmt.execute(params![site, xid, series])?;
        Ok(())
    }

    pub fn add_merge(&mut self, series: i64, into: i64) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare_cached("INSERT OR REPLACE INTO merged (src, dst) VALUES (?1, ?2)")?;
        stmt.execute(params![series, into])?;
        Ok(())
    }

    /// Repoints index entries of merged series at their final series, drops dangling ones,
    /// stamps `built_at` (Unix ms) and `build_id`, and commits.
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

/// A built `metadata.db`, opened read-only.
pub struct MetadataDb {
    conn: Mutex<Connection>,
}

impl MetadataDb {
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
        // Read-only: a panic can't leave anything half-changed.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Unix milliseconds.
    pub fn built_at(&self) -> Result<Option<i64>> {
        Ok(self.meta("built_at")?.and_then(|v| v.parse().ok()))
    }

    /// Unique per build.
    pub fn build_id(&self) -> Result<String> {
        Ok(self.meta("build_id")?.unwrap_or_default())
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    /// `None` for a merged, deleted or unknown series.
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

    pub fn by_title_key(&self, key: &str) -> Result<Vec<i64>> {
        self.ids(
            "SELECT DISTINCT series FROM titles WHERE key = ?1 ORDER BY series",
            &[key],
        )
    }

    pub fn by_link_key(&self, key: &str) -> Result<Vec<i64>> {
        self.ids(
            "SELECT DISTINCT series FROM links WHERE key = ?1 ORDER BY series",
            &[key],
        )
    }

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
