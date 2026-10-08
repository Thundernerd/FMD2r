//! `lists.db`: one master list of manga for every module, with an FTS5 index.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{Row, Statement, params, params_from_iter};

use crate::db::Db;
use crate::error::Result;

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/lists_v1.sql"),
    include_str!("migrations/lists_v2.sql"),
];

/// Records that `module_id`'s list changed now, inside the transaction that changed it.
const MARK_UPDATED: &str = "INSERT INTO list_updates (module_id, updated_at)
    VALUES (?1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (module_id) DO UPDATE SET updated_at = excluded.updated_at";

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

/// How many listings carry one genre or status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetCount {
    pub value: String,
    pub count: u64,
}

/// The genres and statuses of a set of listings, from [`MasterListRepo::facets`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {
    pub genres: Vec<FacetCount>,
    pub statuses: Vec<FacetCount>,
}

/// A module's list at a glance, from [`MasterListRepo::summaries`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSummary {
    pub module_id: String,
    /// Titles listed.
    pub count: u64,
    /// When the list last changed through an update or import, in Unix milliseconds.
    pub updated_at: Option<i64>,
}

/// Repository for the master list. Obtain it with [`ListsDb::masterlist`].
pub struct MasterListRepo<'a> {
    db: &'a Db,
}

/// The per-row insert/delete triggers that keep `masterlist_fts` in sync.
const BULK_TRIGGERS: [&str; 2] = ["masterlist_ai", "masterlist_ad"];

const INSERT: &str = "INSERT INTO masterlist
    (module_id, link, title, alttitles, authors, artists, genres, status, summary, numchapter, added_jdn)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)";

/// Inserts `l`; returns the number of rows changed (0 when a conflict clause skipped it).
fn execute_insert(stmt: &mut Statement<'_>, module_id: &str, l: &MangaListing) -> Result<usize> {
    Ok(stmt.execute(params![
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
    ])?)
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
    /// Replaces the whole list of `module_id` with `rows` in one transaction. This is the bulk
    /// import path: the per-row FTS triggers are dropped for the duration of the transaction (and
    /// recreated from their stored SQL before commit, or restored by the rollback on error), the
    /// rows go through one prepared statement, and the index is synced with two set-based
    /// statements. This is several times faster than letting the triggers fire per row.
    pub fn replace_module<I>(&self, module_id: &str, rows: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Borrow<MangaListing>,
    {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        let triggers: Vec<(String, String)> = {
            let mut stmt = tx.prepare(
                "SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND name IN (?1, ?2)",
            )?;
            let rows = stmt.query_map(BULK_TRIGGERS, |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (name, _) in &triggers {
            tx.execute_batch(&format!("DROP TRIGGER \"{name}\""))?;
        }
        tx.execute(
            "INSERT INTO masterlist_fts
                (masterlist_fts, rowid, title, alttitles, authors, artists, genres, summary)
             SELECT 'delete', id, title, alttitles, authors, artists, genres, summary
             FROM masterlist WHERE module_id = ?1",
            [module_id],
        )?;
        tx.execute("DELETE FROM masterlist WHERE module_id = ?1", [module_id])?;
        {
            // Duplicate links keep the first row, like FMD2's `INSERT OR IGNORE`
            // (baseunits/DBDataProcess.pas:1089).
            let mut stmt = tx.prepare_cached(&format!("{INSERT} ON CONFLICT DO NOTHING"))?;
            for row in rows {
                execute_insert(&mut stmt, module_id, row.borrow())?;
            }
        }
        tx.execute(
            "INSERT INTO masterlist_fts (rowid, title, alttitles, authors, artists, genres, summary)
             SELECT id, title, alttitles, authors, artists, genres, summary
             FROM masterlist WHERE module_id = ?1",
            [module_id],
        )?;
        for (_, sql) in &triggers {
            tx.execute_batch(sql)?;
        }
        tx.execute(MARK_UPDATED, [module_id])?;
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
        execute_insert(&mut stmt, module_id, listing)?;
        Ok(())
    }

    /// Adds the listings whose link `module_id` does not list yet, in one transaction, and keeps
    /// the stored ones as they are, like FMD2's `INSERT OR IGNORE` (baseunits/DBDataProcess.pas:1089).
    /// Returns how many were added.
    pub fn insert_new<I>(&self, module_id: &str, rows: I) -> Result<u64>
    where
        I: IntoIterator,
        I::Item: Borrow<MangaListing>,
    {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        let mut added = 0;
        {
            let mut stmt = tx.prepare_cached(&format!("{INSERT} ON CONFLICT DO NOTHING"))?;
            for row in rows {
                added += execute_insert(&mut stmt, module_id, row.borrow())? as u64;
            }
        }
        tx.execute(MARK_UPDATED, [module_id])?;
        tx.commit()?;
        Ok(added)
    }

    /// Every link `module_id` lists.
    pub fn links(&self, module_id: &str) -> Result<HashSet<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("SELECT link FROM masterlist WHERE module_id = ?1")?;
        let links = stmt
            .query_map([module_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(links)
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
        let (where_clause, mut args) = where_clause(query, filters);

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

impl MasterListRepo<'_> {
    /// How many of the listings [`MasterListRepo::search`] matches carry each genre and each
    /// status. `genres` is FMD2's comma-separated list (e.g. `Action, Comedy`); each trimmed,
    /// non-empty item counts once per listing. Both are ordered by count, then name.
    pub fn facets(&self, query: &str, filters: &SearchFilters) -> Result<Facets> {
        let (where_clause, args) = where_clause(query, filters);
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT m.genres, m.status FROM masterlist m {where_clause}"
        ))?;
        let mut rows = stmt.query(params_from_iter(&args))?;
        let mut genres: HashMap<String, u64> = HashMap::new();
        let mut statuses: HashMap<String, u64> = HashMap::new();
        while let Some(row) = rows.next()? {
            let row_genres: &str = row.get_ref(0)?.as_str().unwrap_or_default();
            let mut seen: HashSet<&str> = HashSet::new();
            for genre in row_genres
                .split(',')
                .map(str::trim)
                .filter(|g| !g.is_empty())
            {
                if seen.insert(genre) {
                    *genres.entry(genre.to_owned()).or_default() += 1;
                }
            }
            let status: String = row.get(1)?;
            *statuses.entry(status).or_default() += 1;
        }
        Ok(Facets {
            genres: sorted_counts(genres),
            statuses: sorted_counts(statuses),
        })
    }

    /// The size and last change of every module's list, by module ID.
    pub fn summaries(&self) -> Result<Vec<ListSummary>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT module_id, SUM(n), MAX(updated_at) FROM (
                 SELECT module_id, COUNT(*) AS n, NULL AS updated_at
                 FROM masterlist GROUP BY module_id
                 UNION ALL
                 SELECT module_id, 0, updated_at FROM list_updates
             ) GROUP BY module_id ORDER BY module_id",
        )?;
        let summaries = stmt
            .query_map([], |r| {
                Ok(ListSummary {
                    module_id: r.get(0)?,
                    count: r.get(1)?,
                    updated_at: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(summaries)
    }
}

/// `(value, count)` pairs by descending count, then value.
fn sorted_counts(counts: HashMap<String, u64>) -> Vec<FacetCount> {
    let mut counts: Vec<FacetCount> = counts
        .into_iter()
        .map(|(value, count)| FacetCount { value, count })
        .collect();
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    counts
}

/// The `WHERE` clause (empty when nothing filters) and its arguments for
/// [`MasterListRepo::search`] and [`MasterListRepo::facets`], over `masterlist m`.
fn where_clause(query: &str, filters: &SearchFilters) -> (String, Vec<Value>) {
    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<Value> = Vec::new();

    if let Some(fts) = fts_query(query) {
        conds
            .push("m.id IN (SELECT rowid FROM masterlist_fts WHERE masterlist_fts MATCH ?)".into());
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
    if conds.is_empty() {
        (String::new(), args)
    } else {
        (format!("WHERE {}", conds.join(" AND ")), args)
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

/// Reads the list in an FMD2 per-site database (`<module id>.db`, as FMD2-DB ships them): the
/// `masterlist` table of baseunits/DBDataProcess.pas:143-153, `jdn` read as `added_jdn`. FMD2
/// leaves SQLite's type affinity alone, so a value of the wrong type converts the way SQLite
/// casts it, NULL becomes empty or 0, and text that is not UTF-8 is decoded lossily.
pub fn read_fmd2_list(path: impl AsRef<Path>) -> Result<Vec<MangaListing>> {
    let conn = rusqlite::Connection::open_with_flags(
        path.as_ref(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut stmt = conn.prepare(
        "SELECT link, title, alttitles, authors, artists, genres, status, summary,
                CAST(numchapter AS INTEGER), CAST(jdn AS INTEGER)
         FROM masterlist",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MangaListing {
                link: lossy_text(r, 0)?,
                title: lossy_text(r, 1)?,
                alttitles: lossy_text(r, 2)?,
                authors: lossy_text(r, 3)?,
                artists: lossy_text(r, 4)?,
                genres: lossy_text(r, 5)?,
                status: lossy_text(r, 6)?,
                summary: lossy_text(r, 7)?,
                numchapter: r
                    .get::<_, Option<i64>>(8)?
                    .map_or(0, |n| u32::try_from(n).unwrap_or(0)),
                added_jdn: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Column `i` as text: NULL is empty, numbers are formatted, bytes decoded lossily.
fn lossy_text(row: &Row<'_>, i: usize) -> rusqlite::Result<String> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(i)? {
        ValueRef::Null => String::new(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(x) => x.to_string(),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            String::from_utf8_lossy(bytes).into_owned()
        }
    })
}
