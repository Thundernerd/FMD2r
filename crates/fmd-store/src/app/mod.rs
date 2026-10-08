//! `app.db`: application state (tasks, favorites, settings, accounts, events, module files).

pub(crate) mod accounts;
pub(crate) mod downloaded_chapters;
pub(crate) mod tasks;

use std::path::Path;

use crate::crypto::Cipher;
use crate::db::Db;
use crate::error::Result;
use accounts::AccountRepo;
use downloaded_chapters::DownloadedChaptersRepo;
use tasks::TaskRepo;

const MIGRATIONS: &[&str] = &[include_str!("../migrations/app_v1.sql")];

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

    /// Download tasks with their chapters and pages.
    pub fn tasks(&self) -> TaskRepo<'_> {
        TaskRepo::new(&self.db)
    }

    /// Chapters already downloaded, per manga.
    pub fn downloaded_chapters(&self) -> DownloadedChaptersRepo<'_> {
        DownloadedChaptersRepo::new(&self.db)
    }

    /// Module accounts; credentials are encrypted and decrypted with `cipher`.
    pub fn accounts<'a>(&'a self, cipher: &'a dyn Cipher) -> AccountRepo<'a> {
        AccountRepo::new(&self.db, cipher)
    }
}
