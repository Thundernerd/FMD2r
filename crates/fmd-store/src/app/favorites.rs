//! Favorites (the library), replacing FMD2's `favorites` table (baseunits/FavoritesDB.pas:55-71);
//! its `downloadedchapterlist` lives in `downloaded_chapters`.

use rusqlite::{OptionalExtension, Row, params};

use crate::db::Db;
use crate::error::Result;
use crate::sql::{next_sort_order, now_ms, reorder};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FavoriteId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFavorite {
    pub module_id: String,
    pub link: String,
    pub title: String,
    pub save_to: String,
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub id: FavoriteId,
    pub module_id: String,
    pub link: String,
    pub title: String,
    /// As the module reports it.
    pub status: String,
    /// Number of chapters seen at the last check (FMD2's `currentchapter`).
    pub current_chapter: u32,
    pub save_to: String,
    pub enabled: bool,
    pub sort_order: i64,
    /// Unix milliseconds.
    pub date_added: i64,
    /// Unix milliseconds.
    pub date_last_checked: Option<i64>,
    /// Unix milliseconds.
    pub date_last_updated: Option<i64>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFavorite {
    pub favorite: NewFavorite,
    pub status: String,
    pub current_chapter: u32,
    pub enabled: bool,
    /// Unix milliseconds.
    pub date_added: i64,
    /// Unix milliseconds.
    pub date_last_checked: Option<i64>,
    /// Unix milliseconds.
    pub date_last_updated: Option<i64>,
}

const COLUMNS: &str = "id, module_id, link, title, status, current_chapter, save_to, enabled, \
     sort_order, date_added, date_last_checked, date_last_updated, cover_url";

fn favorite_from_row(row: &Row<'_>) -> rusqlite::Result<Favorite> {
    Ok(Favorite {
        id: FavoriteId(row.get(0)?),
        module_id: row.get(1)?,
        link: row.get(2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        current_chapter: row.get(5)?,
        save_to: row.get(6)?,
        enabled: row.get(7)?,
        sort_order: row.get(8)?,
        date_added: row.get(9)?,
        date_last_checked: row.get(10)?,
        date_last_updated: row.get(11)?,
        cover_url: row.get(12)?,
    })
}

pub struct FavoriteRepo<'a> {
    db: &'a Db,
}

impl<'a> FavoriteRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Appends an enabled favorite; fails on a duplicate module and link.
    pub fn create(&self, new: &NewFavorite) -> Result<Favorite> {
        let conn = self.db.lock();
        Ok(conn.query_row(
            &format!(
                "INSERT INTO favorites (module_id, link, title, save_to, cover_url, sort_order, date_added)
                 VALUES (?1, ?2, ?3, ?4, ?5, {}, ?6)
                 RETURNING {COLUMNS}",
                next_sort_order("favorites")
            ),
            params![
                new.module_id,
                new.link,
                new.title,
                new.save_to,
                new.cover_url,
                now_ms()
            ],
            favorite_from_row,
        )?)
    }

    /// Appends with state and dates as given; fails on a duplicate module and link.
    pub fn import(&self, imported: &ImportedFavorite) -> Result<Favorite> {
        let conn = self.db.lock();
        let new = &imported.favorite;
        Ok(conn.query_row(
            &format!(
                "INSERT INTO favorites (module_id, link, title, save_to, cover_url, status,
                    current_chapter, enabled, sort_order, date_added, date_last_checked,
                    date_last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, {}, ?9, ?10, ?11)
                 RETURNING {COLUMNS}",
                next_sort_order("favorites")
            ),
            params![
                new.module_id,
                new.link,
                new.title,
                new.save_to,
                new.cover_url,
                imported.status,
                imported.current_chapter,
                imported.enabled,
                imported.date_added,
                imported.date_last_checked,
                imported.date_last_updated
            ],
            favorite_from_row,
        )?)
    }

    pub fn get(&self, id: FavoriteId) -> Result<Option<Favorite>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM favorites WHERE id = ?1"),
                [id.0],
                favorite_from_row,
            )
            .optional()?)
    }

    pub fn find(&self, module_id: &str, link: &str) -> Result<Option<Favorite>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM favorites WHERE module_id = ?1 AND link = ?2"),
                [module_id, link],
                favorite_from_row,
            )
            .optional()?)
    }

    /// In display order.
    pub fn list(&self) -> Result<Vec<Favorite>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM favorites ORDER BY sort_order"
        ))?;
        let rows = stmt.query_map([], favorite_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Leaves `sort_order` and `date_added` as stored; see [`Self::reorder`].
    pub fn update(&self, favorite: &Favorite) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE favorites SET module_id = ?2, link = ?3, title = ?4, status = ?5,
                current_chapter = ?6, save_to = ?7, enabled = ?8, date_last_checked = ?9,
                date_last_updated = ?10, cover_url = ?11
             WHERE id = ?1",
            params![
                favorite.id.0,
                favorite.module_id,
                favorite.link,
                favorite.title,
                favorite.status,
                favorite.current_chapter,
                favorite.save_to,
                favorite.enabled,
                favorite.date_last_checked,
                favorite.date_last_updated,
                favorite.cover_url
            ],
        )?;
        Ok(())
    }

    /// Puts `ids` first; the rest keep their relative order.
    pub fn reorder(&self, ids: &[FavoriteId]) -> Result<()> {
        reorder(&mut self.db.lock(), "favorites", ids.iter().map(|id| id.0))
    }

    /// The links the site listed at the last check, in its order. FMD2 keeps only their count
    /// (`currentchapter`, baseunits/FavoritesDB.pas:64).
    pub fn set_chapter_links(&self, id: FavoriteId, links: &[&str]) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM favorite_chapters WHERE favorite_id = ?1",
            [id.0],
        )?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO favorite_chapters (favorite_id, position, link)
                 SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM favorites WHERE id = ?1)",
            )?;
            for (position, link) in links.iter().enumerate() {
                stmt.execute(params![id.0, position, link])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn chapter_links(&self, id: FavoriteId) -> Result<Vec<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT link FROM favorite_chapters WHERE favorite_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([id.0], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stored chapter links not in `downloaded_chapters` (case-insensitive, like its key). With
    /// no stored links (e.g. imported from FMD2), the chapter count less the marks.
    pub fn new_chapter_count(&self, favorite: &Favorite) -> Result<u32> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT COUNT(*), COUNT(*) FILTER (WHERE NOT EXISTS (
                SELECT 1 FROM downloaded_chapters d
                WHERE d.module_id = ?2 AND d.manga_link = ?3 AND d.chapter_link = c.link))
             FROM favorite_chapters c WHERE c.favorite_id = ?1",
        )?;
        let (stored, unmarked): (u32, u32) = stmt.query_row(
            params![favorite.id.0, favorite.module_id, favorite.link],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if stored > 0 {
            return Ok(unmarked);
        }
        let mut stmt = conn.prepare_cached(
            "SELECT COUNT(*) FROM downloaded_chapters WHERE module_id = ?1 AND manga_link = ?2",
        )?;
        let marked: u32 =
            stmt.query_row(params![favorite.module_id, favorite.link], |r| r.get(0))?;
        Ok(favorite.current_chapter.saturating_sub(marked))
    }

    pub fn delete(&self, id: FavoriteId) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM favorites WHERE id = ?1", [id.0])?;
        Ok(())
    }
}
