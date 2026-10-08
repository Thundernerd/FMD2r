//! `app.db`: application state (tasks, favorites, settings, accounts, events, module files).

pub(crate) mod accounts;
pub(crate) mod downloaded_chapters;
pub(crate) mod events;
pub(crate) mod favorites;
pub(crate) mod module_files;
pub(crate) mod module_settings;
pub(crate) mod settings;
pub(crate) mod tasks;

use std::path::Path;

use crate::crypto::Cipher;
use crate::db::Db;
use crate::error::Result;
use accounts::AccountRepo;
use downloaded_chapters::DownloadedChaptersRepo;
use events::EventRepo;
use favorites::FavoriteRepo;
use module_files::ModuleFileRepo;
use module_settings::ModuleSettingsRepo;
use settings::SettingsRepo;
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

    /// Inbox and history events.
    pub fn events(&self) -> EventRepo<'_> {
        EventRepo::new(&self.db)
    }

    /// Per-module options, HTTP and limit overrides, and cookie jars.
    pub fn module_settings(&self) -> ModuleSettingsRepo<'_> {
        ModuleSettingsRepo::new(&self.db)
    }

    /// Application settings (key → JSON).
    pub fn settings(&self) -> SettingsRepo<'_> {
        SettingsRepo::new(&self.db)
    }

    /// Lua files synced from upstream.
    pub fn module_files(&self) -> ModuleFileRepo<'_> {
        ModuleFileRepo::new(&self.db)
    }

    /// Favorites (the library).
    pub fn favorites(&self) -> FavoriteRepo<'_> {
        FavoriteRepo::new(&self.db)
    }
}
