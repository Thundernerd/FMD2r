//! Favorites (the library), replacing FMD2's `favorites` table (baseunits/FavoritesDB.pas:55-71);
//! its `downloadedchapterlist` lives in `downloaded_chapters`.

use std::collections::HashSet;

use rusqlite::{OptionalExtension, Row, params};

use crate::db::Db;
use crate::error::Result;
use crate::lists::MatchInput;
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

/// What the website says about a favorite, as MangaBaka matching reads it. Apart from
/// [`Favorite::title`], which the user can rename; empty until the website's info is fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FavoriteInfo {
    pub title: String,
    pub alt_titles: String,
    pub authors: String,
    pub artists: String,
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

/// The title matching reads: a favorite whose info was never fetched (imported) matches on its
/// title.
macro_rules! match_title {
    () => {
        "IIF(info_title = '', title, info_title)"
    };
}

/// What matching reads of a favorite, plus the database build ID in `?2`, joined like the list
/// titles' fingerprint.
const FINGERPRINT: &str = concat!(
    match_title!(),
    " || char(31) || alt_titles || char(31) || authors || char(31) || artists || char(31) || ?2"
);

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

    pub fn info(&self, id: FavoriteId) -> Result<Option<FavoriteInfo>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                "SELECT info_title, alt_titles, authors, artists FROM favorites WHERE id = ?1",
                [id.0],
                |r| {
                    Ok(FavoriteInfo {
                        title: r.get(0)?,
                        alt_titles: r.get(1)?,
                        authors: r.get(2)?,
                        artists: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Returns whether it changed what was stored.
    pub fn set_info(&self, id: FavoriteId, info: &FavoriteInfo) -> Result<bool> {
        let conn = self.db.lock();
        let changed = conn.execute(
            "UPDATE favorites SET info_title = ?2, alt_titles = ?3, authors = ?4, artists = ?5
             WHERE id = ?1
             AND (info_title, alt_titles, authors, artists) IS NOT (?2, ?3, ?4, ?5)",
            params![
                id.0,
                info.title,
                info.alt_titles,
                info.authors,
                info.artists
            ],
        )?;
        Ok(changed > 0)
    }

    /// `module_id`'s favorites as MangaBaka matching reads them, fingerprinted against database
    /// build `build_id`.
    pub fn match_inputs(&self, module_id: &str, build_id: &str) -> Result<Vec<MatchInput>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT link, {}, alt_titles, authors, artists, {FINGERPRINT}
             FROM favorites WHERE module_id = ?1 ORDER BY id",
            match_title!()
        ))?;
        let rows = stmt.query_map([module_id, build_id], |r| {
            Ok(MatchInput {
                link: r.get(0)?,
                title: r.get(1)?,
                alttitles: r.get(2)?,
                authors: r.get(3)?,
                artists: r.get(4)?,
                fingerprint: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn links(&self, module_id: &str) -> Result<HashSet<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached("SELECT link FROM favorites WHERE module_id = ?1")?;
        let rows = stmt.query_map([module_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The modules with favorites.
    pub fn module_ids(&self) -> Result<Vec<String>> {
        let conn = self.db.lock();
        let mut stmt =
            conn.prepare_cached("SELECT DISTINCT module_id FROM favorites ORDER BY module_id")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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
