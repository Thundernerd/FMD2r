//! `lists.db`: one master list of manga for every module, with an FTS5 index.

use std::borrow::Borrow;
use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{Row, Statement, params, params_from_iter};

use crate::db::Db;
use crate::error::Result;

const MIGRATIONS: &[&str] = &[include_str!("migrations/lists_v1.sql")];

/// Handle to `lists.db`. Clone it to share between threads.
#[derive(Clone)]
pub struct ListsDb {
    db: Db,
}

impl ListsDb {
    /// Opens `lists.db` at `path`, creating it and running pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db: Db::open(path.as_ref(), "lists.db", MIGRATIONS)?,
        })
    }

    /// The schema version recorded in the database (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<u32> {
        self.db.schema_version()
    }

    /// The master list of every module.
    pub fn masterlist(&self) -> MasterListRepo<'_> {
        MasterListRepo { db: &self.db }
    }
}

/// One manga in a module's list: the columns of FMD2's per-site list table
/// (baseunits/DBDataProcess.pas:143-153), with `jdn` renamed `added_jdn`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MangaListing {
    pub link: String,
    pub title: String,
    pub alttitles: String,
    pub authors: String,
    pub artists: String,
    pub genres: String,
    pub status: String,
    pub summary: String,
    pub numchapter: u32,
    /// Julian day number of the day the manga was first listed.
    pub added_jdn: i64,
}

/// A [`MangaListing`] together with the module it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterListEntry {
    pub module_id: String,
    pub listing: MangaListing,
}

/// Filters applied on top of the text query by [`MasterListRepo::search`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilters {
    /// Only these modules; empty means all modules.
    pub module_ids: Vec<String>,
    /// Every one of these must occur in `genres`.
    pub include_genres: Vec<String>,
    /// None of these may occur in `genres`.
    pub exclude_genres: Vec<String>,
    /// Exact `status` value.
    pub status: Option<String>,
}

/// Which slice of the ordered results to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    pub offset: u32,
    pub limit: u32,
}

/// One page of search results and the number of matches across all pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResults {
    pub total: u64,
    pub entries: Vec<MasterListEntry>,
}

/// Repository for the master list. Obtain it with [`ListsDb::masterlist`].
pub struct MasterListRepo<'a> {
    db: &'a Db,
}

const INSERT: &str = "INSERT INTO masterlist
    (module_id, link, title, alttitles, authors, artists, genres, status, summary, numchapter, added_jdn)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)";

fn execute_insert(stmt: &mut Statement<'_>, module_id: &str, l: &MangaListing) -> Result<()> {
    stmt.execute(params![
        module_id,
        l.link,
        l.title,
        l.alttitles,
        l.authors,
        l.artists,
        l.genres,
        l.status,
        l.summary,
        l.numchapter,
        l.added_jdn
    ])?;
    Ok(())
}

fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<MasterListEntry> {
    Ok(MasterListEntry {
        module_id: row.get(0)?,
        listing: MangaListing {
            link: row.get(1)?,
            title: row.get(2)?,
            alttitles: row.get(3)?,
            authors: row.get(4)?,
            artists: row.get(5)?,
            genres: row.get(6)?,
            status: row.get(7)?,
            summary: row.get(8)?,
            numchapter: row.get(9)?,
            added_jdn: row.get(10)?,
        },
    })
}

impl MasterListRepo<'_> {
    /// Replaces the whole list of `module_id` with `rows` in one transaction (the bulk import
    /// path: one prepared statement, FTS kept in sync by triggers).
    pub fn replace_module<I>(&self, module_id: &str, rows: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<MangaListing>,
    {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM masterlist WHERE module_id = ?1", [module_id])?;
        {
            let mut stmt = tx.prepare_cached(INSERT)?;
            for row in rows {
                execute_insert(&mut stmt, module_id, row.borrow())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Inserts the listing, or overwrites the stored one with the same (`module_id`, `link`).
    pub fn upsert(&self, module_id: &str, listing: &MangaListing) -> Result<()> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "{INSERT} ON CONFLICT (module_id, link) DO UPDATE SET
                title = excluded.title, alttitles = excluded.alttitles,
                authors = excluded.authors, artists = excluded.artists,
                genres = excluded.genres, status = excluded.status,
                summary = excluded.summary, numchapter = excluded.numchapter,
                added_jdn = excluded.added_jdn"
        ))?;
        execute_insert(&mut stmt, module_id, listing)
    }

    /// Number of listings of `module_id`, or of every module for `None`.
    pub fn count(&self, module_id: Option<&str>) -> Result<u64> {
        let conn = self.db.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM masterlist WHERE ?1 IS NULL OR module_id = ?1",
            [module_id],
            |r| r.get(0),
        )?)
    }

    /// Searches titles and alternative titles (as FMD2's `Search`, baseunits/DBDataProcess.pas:1255)
    /// through the FTS index: every word of `query` must match the start of a word, so partial
    /// input like `one pi` finds "One Piece". An empty query matches everything.
    ///
    /// Genre filters match substrings of the `genres` column like FMD2's `Filter`
    /// (baseunits/DBDataProcess.pas:1367): included genres must all occur, excluded ones must not.
    /// Results are ordered by title, then module and link, so pages are stable.
    pub fn search(
        &self,
        query: &str,
        filters: &SearchFilters,
        page: PageRequest,
    ) -> Result<SearchResults> {
        let mut conds: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();

        if let Some(fts) = fts_query(query) {
            conds.push(
                "m.id IN (SELECT rowid FROM masterlist_fts WHERE masterlist_fts MATCH ?)".into(),
            );
            args.push(fts.into());
        }
        if !filters.module_ids.is_empty() {
            let marks = vec!["?"; filters.module_ids.len()].join(", ");
            conds.push(format!("m.module_id IN ({marks})"));
            args.extend(filters.module_ids.iter().cloned().map(Value::from));
        }
        for (genres, op) in [
            (&filters.include_genres, "LIKE"),
            (&filters.exclude_genres, "NOT LIKE"),
        ] {
            for genre in genres {
                conds.push(format!("m.genres {op} ? ESCAPE '\\'"));
                args.push(format!("%{}%", escape_like(genre)).into());
            }
        }
        if let Some(status) = &filters.status {
            conds.push("m.status = ?".into());
            args.push(status.clone().into());
        }
        let where_clause = if conds.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conds.join(" AND "))
        };

        let conn = self.db.lock();
        let total = conn.query_row(
            &format!("SELECT COUNT(*) FROM masterlist m {where_clause}"),
            params_from_iter(&args),
            |r| r.get(0),
        )?;
        args.push(page.limit.into());
        args.push(page.offset.into());
        let mut stmt = conn.prepare(&format!(
            "SELECT m.module_id, m.link, m.title, m.alttitles, m.authors, m.artists, m.genres,
                    m.status, m.summary, m.numchapter, m.added_jdn
             FROM masterlist m {where_clause}
             ORDER BY m.title COLLATE NOCASE, m.module_id, m.link
             LIMIT ? OFFSET ?"
        ))?;
        let entries = stmt
            .query_map(params_from_iter(&args), entry_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(SearchResults { total, entries })
    }
}

/// Turns user input into an FTS5 query over `title` and `alttitles`: each word becomes a quoted
/// prefix term, so FTS5 operators in the input are treated as text. `None` when the input has no
/// searchable characters.
fn fts_query(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| format!("{{title alttitles}} : ({})", terms.join(" ")))
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
