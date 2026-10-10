//! rusqlite schema, migrations and repositories (`app.db`, `lists.db`).
//!
//! Repository methods block on a shared connection: call them from worker threads or inside
//! `tokio::task::spawn_blocking`.

mod app;
mod cover_links;
mod crypto;
mod db;
mod error;
mod lists;
mod metadata;
mod sql;

pub use app::AppDb;
pub use app::accounts::{Account, AccountRepo, AccountStatus};
pub use app::downloaded_chapters::DownloadedChaptersRepo;
pub use app::events::{Event, EventId, EventQuery, EventRepo, EventSeverity, NewEvent};
pub use app::favorites::{Favorite, FavoriteId, FavoriteRepo, ImportedFavorite, NewFavorite};
pub use app::module_files::{ModuleFile, ModuleFileRepo};
pub use app::module_settings::{ModuleSettings, ModuleSettingsRepo};
pub use app::sessions::SessionRepo;
pub use app::settings::SettingsRepo;
pub use app::tasks::{
    ChapterStatus, ImportedChapter, ImportedTask, NewChapter, NewPage, NewTask, PageStatus, Task,
    TaskChapter, TaskId, TaskPage, TaskRepo, TaskStatus,
};
pub use cover_links::{CoverLink, CoverLinkRepo, CoverSource};
pub use crypto::{ACCOUNTS_KEY_FILE, Cipher, KeyFileCipher};
pub use error::{Result, StoreError};
pub use lists::{
    FacetCount, Facets, ListSummary, ListsDb, MangaListing, MasterListEntry, MasterListRepo,
    MatchConfidence, MatchInput, MatchRepo, PageRequest, SearchFilters, SearchResults, StoredMatch,
    UNKNOWN, read_fmd2_list,
};
pub use metadata::{MetadataBuilder, MetadataDb, MetadataSeries};
