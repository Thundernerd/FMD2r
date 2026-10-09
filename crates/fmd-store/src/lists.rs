//! `lists.db`: one master list of manga for every module, with an FTS5 index.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Row, Statement, params, params_from_iter};

use crate::db::Db;
use crate::error::Result;

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/lists_v1.sql"),
    include_str!("migrations/lists_v2.sql"),
    include_str!("migrations/lists_v3.sql"),
];

/// Run inside the transaction that changed `module_id`'s list.
const MARK_UPDATED: &str = "INSERT INTO list_updates (module_id, updated_at)
    VALUES (?1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (module_id) DO UPDATE SET updated_at = excluded.updated_at";

/// Handle to `lists.db`.
#[derive(Clone)]
pub struct ListsDb {
    db: Db,
}

impl ListsDb {
    /// Opens or creates `lists.db` and runs pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db: Db::open(path.as_ref(), "lists.db", MIGRATIONS)?,
        })
    }

    /// `PRAGMA user_version`.
    pub fn schema_version(&self) -> Result<u32> {
        self.db.schema_version()
    }

    pub fn masterlist(&self) -> MasterListRepo<'_> {
        MasterListRepo { db: &self.db }
    }

    /// The list titles' MangaBaka matches.
    pub fn matches(&self) -> MatchRepo<'_> {
        MatchRepo { db: &self.db }
    }
}

/// The columns of FMD2's per-site list table (baseunits/DBDataProcess.pas:143-153), with `jdn`
/// renamed `added_jdn`.
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
    /// Julian day number of when the manga was first listed.
    pub added_jdn: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterListEntry {
    pub module_id: String,
    pub listing: MangaListing,
    /// From the accepted MangaBaka match, if any.
    pub format: Option<String>,
    /// From the accepted MangaBaka match, if any.
    pub publication: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilters {
    /// Empty means all modules.
    pub module_ids: Vec<String>,
    /// Every one of these must occur in `genres`.
    pub include_genres: Vec<String>,
    /// None of these may occur in `genres`.
    pub exclude_genres: Vec<String>,
    pub status: Option<String>,
    /// The accepted MangaBaka match's format (`manga`, `manhwa`, `manhua`, `oel`, `other`), or
    /// [`UNKNOWN`].
    pub format: Option<String>,
    /// The accepted MangaBaka match's publication status (`ongoing`, `completed`, `hiatus`,
    /// `cancelled`), or [`UNKNOWN`].
    pub publication: Option<String>,
}

/// [`UNKNOWN`], usable in `concat!`.
macro_rules! unknown {
    () => {
        "unknown"
    };
}

/// Format or publication status when no accepted MangaBaka match gives one.
pub const UNKNOWN: &str = unknown!();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    pub offset: u32,
    pub limit: u32,
}

/// One page of results; `total` counts all pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResults {
    pub total: u64,
    pub entries: Vec<MasterListEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetCount {
    pub value: String,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {
    pub genres: Vec<FacetCount>,
    pub statuses: Vec<FacetCount>,
    /// From accepted MangaBaka matches, [`UNKNOWN`] for the rest.
    pub formats: Vec<FacetCount>,
    /// From accepted MangaBaka matches, [`UNKNOWN`] for the rest.
    pub publications: Vec<FacetCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSummary {
    pub module_id: String,
    pub count: u64,
    /// Last update or import, even one that added nothing, in Unix milliseconds.
    pub updated_at: Option<i64>,
}

pub struct MasterListRepo<'a> {
    db: &'a Db,
}

/// Per-row triggers that keep `masterlist_fts` in sync.
const BULK_TRIGGERS: [&str; 2] = ["masterlist_ai", "masterlist_ad"];

const INSERT: &str = "INSERT INTO masterlist
    (module_id, link, title, alttitles, authors, artists, genres, status, summary, numchapter, added_jdn)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)";

/// Returns 0 when a conflict clause skipped the row.
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
        format: row.get(11)?,
        publication: row.get(12)?,
    })
}

impl MasterListRepo<'_> {
    /// Replaces `module_id`'s whole list in one transaction. For speed, the per-row FTS triggers
    /// are dropped and the index synced set-based; the triggers are recreated before commit (or
    /// restored by rollback).
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

    /// Adds listings with new links and keeps existing ones, like FMD2's `INSERT OR IGNORE`
    /// (baseunits/DBDataProcess.pas:1089). Returns how many were added.
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

    pub fn links(&self, module_id: &str) -> Result<HashSet<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("SELECT link FROM masterlist WHERE module_id = ?1")?;
        let links = stmt
            .query_map([module_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(links)
    }

    /// `None` counts every module.
    pub fn count(&self, module_id: Option<&str>) -> Result<u64> {
        let conn = self.db.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM masterlist WHERE ?1 IS NULL OR module_id = ?1",
            [module_id],
            |r| r.get(0),
        )?)
    }

    /// Searches titles and alt titles like FMD2's `Search` (baseunits/DBDataProcess.pas:1255),
    /// each word as a prefix, so `one pi` finds "One Piece". Genre filters match substrings
    /// like FMD2's `Filter` (baseunits/DBDataProcess.pas:1367). Ordered by title, module and
    /// link so pages are stable.
    pub fn search(
        &self,
        query: &str,
        filters: &SearchFilters,
        page: PageRequest,
    ) -> Result<SearchResults> {
        let (where_clause, mut args) = where_clause(query, filters);

        let conn = self.db.lock();
        let total = conn.query_row(
            &format!("SELECT COUNT(*) FROM masterlist m {MATCH_JOIN} {where_clause}"),
            params_from_iter(&args),
            |r| r.get(0),
        )?;
        args.push(page.limit.into());
        args.push(page.offset.into());
        let mut stmt = conn.prepare(&format!(
            "SELECT m.module_id, m.link, m.title, m.alttitles, m.authors, m.artists, m.genres,
                    m.status, m.summary, m.numchapter, m.added_jdn,
                    IIF(mm.series_id IS NULL, NULL, mm.format),
                    IIF(mm.series_id IS NULL, NULL, mm.status)
             FROM masterlist m {MATCH_JOIN} {where_clause}
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
    /// Counts per genre, status, format and publication over what [`MasterListRepo::search`]
    /// matches. Each genre in the comma-separated list counts once per listing.
    pub fn facets(&self, query: &str, filters: &SearchFilters) -> Result<Facets> {
        let (where_clause, args) = where_clause(query, filters);
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT m.genres, m.status, {FORMAT_VALUE}, {PUBLICATION_VALUE}
             FROM masterlist m {MATCH_JOIN} {where_clause}"
        ))?;
        let mut rows = stmt.query(params_from_iter(&args))?;
        let mut genres: HashMap<String, u64> = HashMap::new();
        let mut statuses: HashMap<String, u64> = HashMap::new();
        let mut formats: HashMap<String, u64> = HashMap::new();
        let mut publications: HashMap<String, u64> = HashMap::new();
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
            *formats.entry(row.get(2)?).or_default() += 1;
            *publications.entry(row.get(3)?).or_default() += 1;
        }
        Ok(Facets {
            genres: sorted_counts(genres),
            statuses: sorted_counts(statuses),
            formats: sorted_counts(formats),
            publications: sorted_counts(publications),
        })
    }

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

/// By descending count, then value.
fn sorted_counts(counts: HashMap<String, u64>) -> Vec<FacetCount> {
    let mut counts: Vec<FacetCount> = counts
        .into_iter()
        .map(|(value, count)| FacetCount { value, count })
        .collect();
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    counts
}

const MATCH_JOIN: &str = "LEFT JOIN metadata_matches mm
    ON mm.module_id = m.module_id AND mm.link = m.link";
/// A listing's format facet value: its accepted match's, else [`UNKNOWN`].
const FORMAT_VALUE: &str = concat!(
    "COALESCE(IIF(mm.series_id IS NULL, NULL, mm.format), '",
    unknown!(),
    "')"
);
/// A listing's publication facet value: its accepted match's, else [`UNKNOWN`].
const PUBLICATION_VALUE: &str = concat!(
    "COALESCE(IIF(mm.series_id IS NULL, NULL, mm.status), '",
    unknown!(),
    "')"
);

/// The `WHERE` clause and arguments over `masterlist m` [`MATCH_JOIN`]ed.
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
    for (value, expr) in [
        (&filters.format, FORMAT_VALUE),
        (&filters.publication, PUBLICATION_VALUE),
    ] {
        if let Some(value) = value {
            conds.push(format!("{expr} = ?"));
            args.push(value.clone().into());
        }
    }
    if conds.is_empty() {
        (String::new(), args)
    } else {
        (format!("WHERE {}", conds.join(" AND ")), args)
    }
}

/// Quotes each word as a prefix term so FTS5 operators in user input are treated as text.
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

/// Reads an FMD2 per-site database's `masterlist` (baseunits/DBDataProcess.pas:143-153). FMD2
/// ignores type affinity, so values are cast as SQLite would, NULL becomes empty or 0, and
/// non-UTF-8 text is decoded lossily.
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

/// How a list title's match in MangaBaka's database was decided (T71's tiers,
/// docs/research/metadata-sources.md, "Confidence threshold").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchConfidence {
    /// The title's link is one of the series' links (WebToons `title_no`).
    Link,
    /// An ID the website gives for the title on another site is one of the series'.
    CrossId,
    /// The title matches, and so does a person the list names.
    TitleAuthor,
    /// The title matches exactly one series, and the list names no people.
    TitleUnique,
    /// Rejected: the title matches, but none of the people the list names do.
    AuthorConflict,
    /// Rejected: the title matches several series.
    Ambiguous,
    /// Rejected: no series matches.
    None,
}

impl MatchConfidence {
    pub const ALL: [Self; 7] = [
        Self::Link,
        Self::CrossId,
        Self::TitleAuthor,
        Self::TitleUnique,
        Self::AuthorConflict,
        Self::Ambiguous,
        Self::None,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::CrossId => "cross-id",
            Self::TitleAuthor => "title+author",
            Self::TitleUnique => "title-unique",
            Self::AuthorConflict => "author-conflict",
            Self::Ambiguous => "ambiguous",
            Self::None => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    pub fn is_accepted(self) -> bool {
        matches!(
            self,
            Self::Link | Self::CrossId | Self::TitleAuthor | Self::TitleUnique
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchInput {
    pub link: String,
    pub title: String,
    pub alttitles: String,
    pub authors: String,
    pub artists: String,
    /// What the title was matched on, and against which database.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMatch {
    /// Only for an accepted confidence.
    pub series_id: Option<i64>,
    pub confidence: MatchConfidence,
    /// `manga`, `manhwa`, `manhua`, `oel` or `other`.
    pub format: Option<String>,
    /// `ongoing`, `completed`, `hiatus` or `cancelled`.
    pub publication: Option<String>,
    pub year: Option<i64>,
}

/// The columns matching reads, plus the database build ID in `?2`.
const FINGERPRINT: &str = "m.title || char(31) || m.alttitles || char(31) || m.authors \
    || char(31) || m.artists || char(31) || ?2";

pub struct MatchRepo<'a> {
    db: &'a Db,
}

impl MatchRepo<'_> {
    /// Titles unmatched, changed since matching, or matched against another database build.
    pub fn pending(&self, module_id: &str, build_id: &str) -> Result<Vec<MatchInput>> {
        self.inputs(
            module_id,
            build_id,
            &format!("AND (mm.link IS NULL OR mm.fingerprint <> {FINGERPRINT})"),
        )
    }

    pub fn all(&self, module_id: &str, build_id: &str) -> Result<Vec<MatchInput>> {
        self.inputs(module_id, build_id, "")
    }

    fn inputs(&self, module_id: &str, build_id: &str, condition: &str) -> Result<Vec<MatchInput>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT m.link, m.title, m.alttitles, m.authors, m.artists, {FINGERPRINT}
             FROM masterlist m {MATCH_JOIN}
             WHERE m.module_id = ?1 {condition}
             ORDER BY m.id"
        ))?;
        let inputs = stmt
            .query_map([module_id, build_id], |r| {
                Ok(MatchInput {
                    link: r.get(0)?,
                    title: r.get(1)?,
                    alttitles: r.get(2)?,
                    authors: r.get(3)?,
                    artists: r.get(4)?,
                    fingerprint: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(inputs)
    }

    pub fn store<'m, I>(&self, module_id: &str, matches: I) -> Result<()>
    where
        I: IntoIterator<Item = (&'m MatchInput, &'m StoredMatch)>,
    {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO metadata_matches
                     (module_id, link, series_id, confidence, format, status, year, fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (input, m) in matches {
                stmt.execute(params![
                    module_id,
                    input.link,
                    m.series_id,
                    m.confidence.as_str(),
                    m.format,
                    m.publication,
                    m.year,
                    input.fingerprint,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn prune(&self, module_id: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "DELETE FROM metadata_matches WHERE module_id = ?1
             AND link NOT IN (SELECT link FROM masterlist WHERE module_id = ?1)",
            [module_id],
        )?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.db.lock().execute("DELETE FROM metadata_matches", [])?;
        Ok(())
    }

    pub fn get(&self, module_id: &str, link: &str) -> Result<Option<StoredMatch>> {
        let conn = self.db.lock();
        let row = conn
            .query_row(
                "SELECT series_id, confidence, format, status, year FROM metadata_matches
                 WHERE module_id = ?1 AND link = ?2",
                [module_id, link],
                |r| {
                    let confidence: String = r.get(1)?;
                    Ok((
                        confidence,
                        StoredMatch {
                            series_id: r.get(0)?,
                            confidence: MatchConfidence::None,
                            format: r.get(2)?,
                            publication: r.get(3)?,
                            year: r.get(4)?,
                        },
                    ))
                },
            )
            .optional()?;
        let Some((confidence, stored)) = row else {
            return Ok(None);
        };
        let confidence =
            MatchConfidence::parse(&confidence).ok_or(crate::StoreError::InvalidColumn {
                column: "metadata_matches.confidence",
                value: confidence,
            })?;
        Ok(Some(StoredMatch {
            confidence,
            ..stored
        }))
    }
}
