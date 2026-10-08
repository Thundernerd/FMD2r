//! rusqlite schema, migrations and repositories (`app.db`, `lists.db`).
//!
//! # Concurrency
//!
//! [`AppDb`] and [`ListsDb`] each own a single SQLite connection behind a `Mutex` and are cheap to
//! clone (`Arc`). Repository methods are blocking: call them directly from worker threads, and
//! wrap them in `tokio::task::spawn_blocking` from async code. Each method holds the lock for one
//! statement or one transaction only. Both databases run in WAL mode with foreign keys on and a
//! busy timeout.

mod app;
mod crypto;
mod db;
mod error;
mod lists;
mod sql;

pub use app::AppDb;
pub use app::accounts::{Account, AccountRepo, AccountStatus};
pub use app::downloaded_chapters::DownloadedChaptersRepo;
pub use app::events::{Event, EventId, EventQuery, EventRepo, EventSeverity, NewEvent};
pub use app::favorites::{Favorite, FavoriteId, FavoriteRepo, NewFavorite};
pub use app::module_files::{ModuleFile, ModuleFileRepo};
pub use app::module_settings::{ModuleSettings, ModuleSettingsRepo};
pub use app::settings::SettingsRepo;
pub use app::tasks::{
    ChapterStatus, NewChapter, NewPage, NewTask, PageStatus, Task, TaskChapter, TaskId, TaskPage,
    TaskRepo, TaskStatus,
};
pub use crypto::{Cipher, KeyFileCipher};
pub use error::{Result, StoreError};
pub use lists::{
    ListsDb, MangaListing, MasterListEntry, MasterListRepo, PageRequest, SearchFilters,
    SearchResults,
};
