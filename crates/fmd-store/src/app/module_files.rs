//! Lua files synced from upstream by the module updater (T29).

use rusqlite::{OptionalExtension, Row, params};

use crate::db::Db;
use crate::error::Result;

/// A synced Lua file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFile {
    /// Path relative to the Lua directory.
    pub path: String,
    /// Git blob SHA of the synced content.
    pub sha: String,
    /// Unix milliseconds.
    pub last_modified: i64,
    pub size: u64,
}

fn file_from_row(row: &Row<'_>) -> rusqlite::Result<ModuleFile> {
    Ok(ModuleFile {
        path: row.get(0)?,
        sha: row.get(1)?,
        last_modified: row.get(2)?,
        size: row.get(3)?,
    })
}

/// Repository for synced module files. Obtain it with [`crate::AppDb::module_files`].
pub struct ModuleFileRepo<'a> {
    db: &'a Db,
}

impl<'a> ModuleFileRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub fn get(&self, path: &str) -> Result<Option<ModuleFile>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                "SELECT path, sha, last_modified, size FROM module_files WHERE path = ?1",
                [path],
                file_from_row,
            )
            .optional()?)
    }

    /// Every file, ordered by path.
    pub fn list(&self) -> Result<Vec<ModuleFile>> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare_cached("SELECT path, sha, last_modified, size FROM module_files ORDER BY path")?;
        let rows = stmt.query_map([], file_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn upsert(&self, file: &ModuleFile) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO module_files (path, sha, last_modified, size) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (path) DO UPDATE SET
                sha = excluded.sha, last_modified = excluded.last_modified, size = excluded.size",
            params![file.path, file.sha, file.last_modified, file.size],
        )?;
        Ok(())
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM module_files WHERE path = ?1", [path])?;
        Ok(())
    }
}
