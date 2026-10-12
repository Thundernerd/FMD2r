//! MangaBaka series picked by hand for a series (issue #249): "this series is MangaBaka series N".

use std::collections::{BTreeSet, HashMap};

use rusqlite::{OptionalExtension, params};

use crate::db::Db;
use crate::error::Result;

pub struct MetadataOverrideRepo<'a> {
    db: &'a Db,
}

impl<'a> MetadataOverrideRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Replaces the series picked before, if any.
    pub fn set(&self, module_id: &str, link: &str, series_id: i64) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO metadata_overrides (module_id, link, series_id) VALUES (?1, ?2, ?3)
             ON CONFLICT (module_id, link) DO UPDATE SET series_id = excluded.series_id",
            params![module_id, link, series_id],
        )?;
        Ok(())
    }

    /// Returns whether there was one.
    pub fn remove(&self, module_id: &str, link: &str) -> Result<bool> {
        let conn = self.db.lock();
        let removed = conn.execute(
            "DELETE FROM metadata_overrides WHERE module_id = ?1 AND link = ?2",
            [module_id, link],
        )?;
        Ok(removed > 0)
    }

    pub fn get(&self, module_id: &str, link: &str) -> Result<Option<i64>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                "SELECT series_id FROM metadata_overrides WHERE module_id = ?1 AND link = ?2",
                [module_id, link],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The module's series IDs, by link.
    pub fn for_module(&self, module_id: &str) -> Result<HashMap<String, i64>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT link, series_id FROM metadata_overrides WHERE module_id = ?1",
        )?;
        let rows = stmt.query_map([module_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The modules with overrides.
    pub fn module_ids(&self) -> Result<BTreeSet<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached("SELECT DISTINCT module_id FROM metadata_overrides")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
