//! Favorites (the library), replacing FMD2's `favorites` table (baseunits/FavoritesDB.pas:55-71).
//! FMD2's `downloadedchapterlist` column lives in `downloaded_chapters` instead.

use rusqlite::{OptionalExtension, Row, params};

use crate::db::Db;
use crate::error::Result;
use crate::sql::{now_ms, reorder};

/// Primary key of a favorite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FavoriteId(pub i64);

/// Fields supplied when adding a favorite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFavorite {
    pub module_id: String,
    pub link: String,
    pub title: String,
    pub save_to: String,
    pub cover_url: Option<String>,
}

/// A stored favorite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub id: FavoriteId,
    pub module_id: String,
    pub link: String,
    pub title: String,
    /// The manga's publication status as the module reports it.
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

/// Repository for favorites. Obtain it with [`crate::AppDb::favorites`].
pub struct FavoriteRepo<'a> {
    db: &'a Db,
}

impl<'a> FavoriteRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Adds an enabled favorite at the end of the list, stamped with the current time. Fails if
    /// the module already has a favorite with this link.
    pub fn create(&self, new: &NewFavorite) -> Result<Favorite> {
        let conn = self.db.lock();
        Ok(conn.query_row(
            &format!(
                "INSERT INTO favorites (module_id, link, title, save_to, cover_url, sort_order, date_added)
                 VALUES (?1, ?2, ?3, ?4, ?5, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM favorites), ?6)
                 RETURNING {COLUMNS}"
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

    /// The favorite with exactly this module id and link.
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

    /// Every favorite in display order.
    pub fn list(&self) -> Result<Vec<Favorite>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM favorites ORDER BY sort_order"
        ))?;
        let rows = stmt.query_map([], favorite_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Overwrites the stored favorite with `favorite.id`. `sort_order` and `date_added` are left
    /// as stored; use [`Self::reorder`] to move favorites.
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

    /// Puts the given favorites first, in the given order; the others keep their relative order
    /// after them.
    pub fn reorder(&self, ids: &[FavoriteId]) -> Result<()> {
        let ids: Vec<i64> = ids.iter().map(|id| id.0).collect();
        reorder(&mut self.db.lock(), "favorites", &ids)
    }

    pub fn delete(&self, id: FavoriteId) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM favorites WHERE id = ?1", [id.0])?;
        Ok(())
    }
}
