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
    /// The library group it is in; `None` is "Ungrouped".
    pub group_id: Option<LibraryGroupId>,
    /// Consecutive checks whose info fetch failed; see [`FavoriteRepo::record_check_failure`].
    pub check_failures: u32,
    /// Why the last of them failed.
    pub last_check_error: Option<String>,
    /// Unix milliseconds of the last of them.
    pub date_last_failed: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LibraryGroupId(pub i64);

/// A named section of the library. A favorite is in at most one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryGroup {
    pub id: LibraryGroupId,
    /// Unique, ignoring case.
    pub name: String,
    pub sort_order: i64,
    /// Whether the library shows the section folded.
    pub collapsed: bool,
    /// Which series in no group it collects; `None` is filled by hand only.
    pub rule: Option<GroupRule>,
}

/// What an auto group collects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupRule {
    /// Series with this tag.
    Tag(String),
    /// Series whose MangaBaka match has this format.
    Format(String),
}

impl GroupRule {
    /// As stored in `rule_kind` and `rule_value`.
    fn columns(&self) -> (&'static str, &str) {
        match self {
            GroupRule::Tag(value) => ("tag", value),
            GroupRule::Format(value) => ("format", value),
        }
    }

    /// `None` for no rule, or a kind this build doesn't know.
    fn from_columns(kind: Option<String>, value: Option<String>) -> Option<Self> {
        let value = value?;
        match kind?.as_str() {
            "tag" => Some(GroupRule::Tag(value)),
            "format" => Some(GroupRule::Format(value)),
            _ => None,
        }
    }
}

/// What the website says about a favorite, as MangaBaka matching reads it, plus its genres. Apart
/// from [`Favorite::title`], which the user can rename; empty until the website's info is fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FavoriteInfo {
    pub title: String,
    pub alt_titles: String,
    pub authors: String,
    pub artists: String,
    /// FMD2's comma-separated genre list. Matching doesn't read it.
    pub genres: String,
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
     sort_order, date_added, date_last_checked, date_last_updated, cover_url, group_id, check_failures, last_check_error, \
     date_last_failed";

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
        group_id: row.get::<_, Option<i64>>(13)?.map(LibraryGroupId),
        check_failures: row.get(14)?,
        last_check_error: row.get(15)?,
        date_last_failed: row.get(16)?,
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
                "SELECT info_title, alt_titles, authors, artists, genres FROM favorites
                 WHERE id = ?1",
                [id.0],
                |r| {
                    Ok(FavoriteInfo {
                        title: r.get(0)?,
                        alt_titles: r.get(1)?,
                        authors: r.get(2)?,
                        artists: r.get(3)?,
                        genres: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Returns whether it changed what matching reads: a change to only the genres is stored but
    /// returns `false`.
    pub fn set_info(&self, id: FavoriteId, info: &FavoriteInfo) -> Result<bool> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE favorites SET genres = ?2 WHERE id = ?1",
            params![id.0, info.genres],
        )?;
        let changed = tx.execute(
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
        tx.commit()?;
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
        update_row(&self.db.lock(), favorite)?;
        Ok(())
    }

    /// Reads, changes and stores each favorite in one transaction, as [`Self::update`] stores
    /// it: all of them, or none and `None` when one of them is not in the library.
    pub fn update_each(
        &self,
        ids: &[FavoriteId],
        mut change: impl FnMut(&mut Favorite),
    ) -> Result<Option<Vec<Favorite>>> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        let mut favorites = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(mut favorite) = tx
                .query_row(
                    &format!("SELECT {COLUMNS} FROM favorites WHERE id = ?1"),
                    [id.0],
                    favorite_from_row,
                )
                .optional()?
            else {
                return Ok(None);
            };
            change(&mut favorite);
            update_row(&tx, &favorite)?;
            favorites.push(favorite);
        }
        tx.commit()?;
        Ok(Some(favorites))
    }

    /// Counts a check whose info fetch failed with `error` at `at` (Unix milliseconds). Beyond
    /// FMD2, which only logs it (`DoCheck`, baseunits/uFavoritesManager.pas:391-394).
    pub fn record_check_failure(&self, id: FavoriteId, error: &str, at: i64) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE favorites SET check_failures = check_failures + 1, last_check_error = ?2,
                date_last_failed = ?3
             WHERE id = ?1",
            params![id.0, error, at],
        )?;
        Ok(())
    }

    /// Forgets the failed checks, after one succeeded.
    pub fn clear_check_failures(&self, id: FavoriteId) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE favorites SET check_failures = 0, last_check_error = NULL,
                date_last_failed = NULL
             WHERE id = ?1",
            [id.0],
        )?;
        Ok(())
    }

    /// Puts the favorite in `group` unless it is in a group already, or gone. Returns whether
    /// it did.
    pub fn group_if_ungrouped(&self, id: FavoriteId, group: LibraryGroupId) -> Result<bool> {
        let conn = self.db.lock();
        let changed = conn.execute(
            "UPDATE favorites SET group_id = ?2 WHERE id = ?1 AND group_id IS NULL",
            params![id.0, group.0],
        )?;
        Ok(changed > 0)
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

    /// Deletes every favorite in one transaction: all of them, or none and `false` when one of
    /// them is not in the library.
    pub fn delete_all(&self, ids: &[FavoriteId]) -> Result<bool> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        for id in ids {
            if delete_row(&tx, *id)? == 0 {
                return Ok(false);
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn delete(&self, id: FavoriteId) -> Result<()> {
        delete_row(&self.db.lock(), id)?;
        Ok(())
    }
}

/// The number of rows changed: 0 when the favorite is not stored.
fn update_row(conn: &rusqlite::Connection, favorite: &Favorite) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE favorites SET module_id = ?2, link = ?3, title = ?4, status = ?5,
            current_chapter = ?6, save_to = ?7, enabled = ?8, date_last_checked = ?9,
            date_last_updated = ?10, cover_url = ?11, group_id = ?12
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
            favorite.cover_url,
            favorite.group_id.map(|g| g.0)
        ],
    )?)
}

/// The number of rows deleted: 0 when the favorite is not stored.
fn delete_row(conn: &rusqlite::Connection, id: FavoriteId) -> Result<usize> {
    Ok(conn.execute("DELETE FROM favorites WHERE id = ?1", [id.0])?)
}

const GROUP_COLUMNS: &str = "id, name, sort_order, collapsed, rule_kind, rule_value";

fn group_from_row(row: &Row<'_>) -> rusqlite::Result<LibraryGroup> {
    Ok(LibraryGroup {
        id: LibraryGroupId(row.get(0)?),
        name: row.get(1)?,
        sort_order: row.get(2)?,
        collapsed: row.get(3)?,
        rule: GroupRule::from_columns(row.get(4)?, row.get(5)?),
    })
}

pub struct LibraryGroupRepo<'a> {
    db: &'a Db,
}

impl<'a> LibraryGroupRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Appends an unfolded group; fails when the name is taken, ignoring case.
    pub fn create(&self, name: &str) -> Result<LibraryGroup> {
        let conn = self.db.lock();
        Ok(conn.query_row(
            &format!(
                "INSERT INTO library_groups (name, sort_order) VALUES (?1, {})
                 RETURNING {GROUP_COLUMNS}",
                next_sort_order("library_groups")
            ),
            [name],
            group_from_row,
        )?)
    }

    pub fn get(&self, id: LibraryGroupId) -> Result<Option<LibraryGroup>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {GROUP_COLUMNS} FROM library_groups WHERE id = ?1"),
                [id.0],
                group_from_row,
            )
            .optional()?)
    }

    /// The group named `name`, ignoring case.
    pub fn find(&self, name: &str) -> Result<Option<LibraryGroup>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {GROUP_COLUMNS} FROM library_groups WHERE name = ?1"),
                [name],
                group_from_row,
            )
            .optional()?)
    }

    /// In display order.
    pub fn list(&self) -> Result<Vec<LibraryGroup>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {GROUP_COLUMNS} FROM library_groups ORDER BY sort_order"
        ))?;
        let rows = stmt.query_map([], group_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stores the name, fold state and rule; fails when the name is taken, ignoring case. Leaves
    /// `sort_order` as stored; see [`Self::reorder`].
    pub fn update(&self, group: &LibraryGroup) -> Result<()> {
        let conn = self.db.lock();
        let (rule_kind, rule_value) = group.rule.as_ref().map(GroupRule::columns).unzip();
        conn.execute(
            "UPDATE library_groups SET name = ?2, collapsed = ?3, rule_kind = ?4, rule_value = ?5
             WHERE id = ?1",
            params![
                group.id.0,
                group.name,
                group.collapsed,
                rule_kind,
                rule_value
            ],
        )?;
        Ok(())
    }

    /// Puts `ids` first; the rest keep their relative order.
    pub fn reorder(&self, ids: &[LibraryGroupId]) -> Result<()> {
        reorder(
            &mut self.db.lock(),
            "library_groups",
            ids.iter().map(|id| id.0),
        )
    }

    /// Its favorites become ungrouped.
    pub fn delete(&self, id: LibraryGroupId) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM library_groups WHERE id = ?1", [id.0])?;
        Ok(())
    }
}
