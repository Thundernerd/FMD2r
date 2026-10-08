//! `lists.db`: one master list of manga for every module, with an FTS5 index.

use std::path::Path;

use crate::db::Db;
use crate::error::Result;

const MIGRATIONS: &[&str] = &[include_str!("migrations/lists_v1.sql")];

/// Handle to `lists.db`. Clone it to share between threads.
#[derive(Clone)]
pub struct ListsDb {
    db: Db,
}

impl ListsDb {
    /// Opens `lists.db` at `path`, creating it and running pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db: Db::open(path.as_ref(), "lists.db", MIGRATIONS)?,
        })
    }

    /// The schema version recorded in the database (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<u32> {
        self.db.schema_version()
    }
}
