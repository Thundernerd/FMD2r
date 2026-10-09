//! `app.db`: application state (tasks, favorites, settings, accounts, events, module files,
//! login sessions).

pub(crate) mod accounts;
pub(crate) mod downloaded_chapters;
pub(crate) mod events;
pub(crate) mod favorites;
pub(crate) mod module_files;
pub(crate) mod module_settings;
pub(crate) mod sessions;
pub(crate) mod settings;
pub(crate) mod tasks;

use std::path::Path;
use std::sync::Arc;

use crate::crypto::{ACCOUNTS_KEY_FILE, Cipher, KeyFileCipher};
use crate::db::Db;
use crate::error::Result;
use accounts::AccountRepo;
use downloaded_chapters::DownloadedChaptersRepo;
use events::EventRepo;
use favorites::FavoriteRepo;
use module_files::ModuleFileRepo;
use module_settings::{ModuleSettings, ModuleSettingsRepo};
use sessions::SessionRepo;
use settings::SettingsRepo;
use tasks::TaskRepo;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/app_v1.sql"),
    include_str!("../migrations/app_v2.sql"),
];

/// The path SQLite opens as a private in-memory database.
const IN_MEMORY: &str = ":memory:";

/// Handle to `app.db`. Clone it to share between threads.
#[derive(Clone)]
pub struct AppDb {
    db: Db,
    cipher: Arc<dyn Cipher>,
}

impl AppDb {
    /// Opens `app.db` at `path`, creating it and running pending migrations.
    ///
    /// Secrets in the database are encrypted with the key in [`ACCOUNTS_KEY_FILE`] next to it,
    /// which is created when missing. An in-memory database (`:memory:`) gets a random key that
    /// lives as long as the process.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let cipher = if path.as_os_str() == IN_MEMORY {
            KeyFileCipher::random()
        } else {
            let dir = path.parent().unwrap_or_else(|| Path::new(""));
            KeyFileCipher::open_or_create(dir.join(ACCOUNTS_KEY_FILE))?
        };
        Ok(Self {
            db: Db::open(path, "app.db", MIGRATIONS)?,
            cipher: Arc::new(cipher),
        })
    }

    /// Encrypts and decrypts the secrets stored in this database: the key of
    /// [`ACCOUNTS_KEY_FILE`] next to it.
    pub fn cipher(&self) -> &dyn Cipher {
        self.cipher.as_ref()
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
        ModuleSettingsRepo::new(&self.db, self.cipher.as_ref())
    }

    /// Application settings (key → JSON).
    pub fn settings(&self) -> SettingsRepo<'_> {
        SettingsRepo::new(&self.db)
    }

    /// Stores application settings (key → JSON) and modules' overrides in one transaction:
    /// either all are written or none. The modules' cookie jars are kept.
    pub fn save_settings(
        &self,
        settings: &[(&str, &serde_json::Value)],
        modules: &[ModuleSettings],
    ) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        for (key, value) in settings {
            settings::put(&tx, key, &serde_json::to_string(value)?)?;
        }
        for module in modules {
            ModuleSettingsRepo::put_overrides(&tx, module)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Web UI login sessions.
    pub fn sessions(&self) -> SessionRepo<'_> {
        SessionRepo::new(&self.db)
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
