//! Chapters already downloaded per manga, normalised from FMD2's one-row-per-manga text blob
//! (baseunits/DownloadedChaptersDB.pas:124-129).

use rusqlite::params;

use crate::db::Db;
use crate::error::Result;

/// Repository for downloaded chapters. Obtain it with [`crate::AppDb::downloaded_chapters`].
///
/// Module ids and links compare case-insensitively, as FMD2 lowercases the key and merges chapter
/// lists with `MergeCaseInsensitive` (baseunits/DownloadedChaptersDB.pas:70-73).
pub struct DownloadedChaptersRepo<'a> {
    db: &'a Db,
}

impl<'a> DownloadedChaptersRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Records `chapter_links` as downloaded for the manga. Already recorded chapters are ignored.
    pub fn mark(&self, module_id: &str, manga_link: &str, chapter_links: &[&str]) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO downloaded_chapters (module_id, manga_link, chapter_link)
                 VALUES (?1, ?2, ?3)",
            )?;
            for chapter in chapter_links {
                stmt.execute(params![module_id, manga_link, chapter])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn contains(&self, module_id: &str, manga_link: &str, chapter_link: &str) -> Result<bool> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM downloaded_chapters
             WHERE module_id = ?1 AND manga_link = ?2 AND chapter_link = ?3)",
        )?;
        Ok(stmt.query_row(params![module_id, manga_link, chapter_link], |r| r.get(0))?)
    }

    /// The manga's downloaded chapter links, sorted case-insensitively.
    pub fn list_for(&self, module_id: &str, manga_link: &str) -> Result<Vec<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT chapter_link FROM downloaded_chapters
             WHERE module_id = ?1 AND manga_link = ?2 ORDER BY chapter_link",
        )?;
        let rows = stmt.query_map(params![module_id, manga_link], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
