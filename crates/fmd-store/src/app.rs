//! `app.db`: application state (tasks, favorites, settings, accounts, events, module files).

use std::path::Path;

use crate::db::Db;
use crate::error::Result;

const MIGRATIONS: &[&str] = &[include_str!("migrations/app_v1.sql")];

/// Handle to `app.db`. Clone it to share between threads.
#[derive(Clone)]
pub struct AppDb {
    db: Db,
}

impl AppDb {
    /// Opens `app.db` at `path`, creating it and running pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db: Db::open(path.as_ref(), "app.db", MIGRATIONS)?,
        })
    }

    /// The schema version recorded in the database (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<u32> {
        self.db.schema_version()
    }
}
