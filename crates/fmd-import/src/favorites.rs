//! `favorites.db` → favorites, with each favorite's `downloadedchapterlist` → downloaded chapters.
//!
//! Schema: `TFavoritesDB.Create` (baseunits/FavoritesDB.pas:55-71); rows are read in `"order"`
//! like `TFavoriteManager.Restore` (:70, baseunits/uFavoritesManager.pas:1301-1340).

use std::collections::HashSet;
use std::path::Path;

use fmd_store::{AppDb, ImportedFavorite, NewFavorite};
use rusqlite::Row;

use crate::ImportOptions;
use crate::error::ImportError;
use crate::fmd2::{chapter_links, datetime, open_db, sql_bool, sql_text, sqlite_error};
use crate::paths::translate;
use crate::report::{ImportReport, SkipReason};

struct FavoriteRow {
    enabled: bool,
    moduleid: String,
    link: String,
    title: String,
    status: String,
    currentchapter: String,
    downloadedchapterlist: String,
    saveto: String,
    dateadded: Option<i64>,
    datelastchecked: Option<i64>,
    datelastupdated: Option<i64>,
}

impl FavoriteRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            enabled: sql_bool(row.get_ref("enabled")?),
            moduleid: sql_text(row.get_ref("moduleid")?),
            link: sql_text(row.get_ref("link")?),
            title: sql_text(row.get_ref("title")?),
            status: sql_text(row.get_ref("status")?),
            currentchapter: sql_text(row.get_ref("currentchapter")?),
            downloadedchapterlist: sql_text(row.get_ref("downloadedchapterlist")?),
            saveto: sql_text(row.get_ref("saveto")?),
            dateadded: datetime(row.get_ref("dateadded")?),
            datelastchecked: datetime(row.get_ref("datelastchecked")?),
            datelastupdated: datetime(row.get_ref("datelastupdated")?),
        })
    }
}

/// The (module id, link) of every favorite in `favorites.db`, to tell module ids from links in
/// `downloadedchapters.db` keys.
pub(crate) fn keys(path: &Path) -> Result<Vec<(String, String)>, ImportError> {
    let Some(conn) = open_db(path)? else {
        return Ok(Vec::new());
    };
    read(&conn)
        .map(|rows| rows.into_iter().map(|f| (f.moduleid, f.link)).collect())
        .map_err(|e| sqlite_error(path, e))
}

fn read(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<FavoriteRow>> {
    let mut stmt = conn.prepare(r#"SELECT * FROM "favorites" ORDER BY "order""#)?;
    let rows = stmt.query_map([], FavoriteRow::from_row)?;
    rows.collect()
}

pub(crate) fn import(
    path: &Path,
    db: &AppDb,
    opts: &ImportOptions,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    let Some(conn) = open_db(path)? else {
        return Ok(());
    };
    report.favorites.found = true;
    let rows = read(&conn).map_err(|e| sqlite_error(path, e))?;

    let mut seen = HashSet::new();
    for f in rows {
        let item = format!("{} {}", f.moduleid, f.link);
        if f.moduleid.is_empty() || f.link.is_empty() {
            report
                .favorites
                .skip(item, SkipReason::Invalid("no module id or link".into()));
            continue;
        }
        // FMD2 merges a favorite's downloaded list into `downloadedchapters.db` as it downloads
        // (baseunits/uFavoritesManager.pas:1131-1133); FMD2r keeps only the latter.
        let chapters = chapter_links(&f.downloadedchapterlist);
        if !opts.dry_run && !chapters.is_empty() {
            db.downloaded_chapters()
                .mark(&f.moduleid, &f.link, &chapters)?;
        }

        let exists = !seen.insert((f.moduleid.clone(), f.link.clone()))
            || db.favorites().find(&f.moduleid, &f.link)?.is_some();
        if exists {
            report.favorites.skip(item, SkipReason::AlreadyExists);
            continue;
        }
        let favorite = ImportedFavorite {
            favorite: NewFavorite {
                save_to: translate(&opts.path_maps, &f.saveto, report),
                module_id: f.moduleid,
                link: f.link,
                title: f.title,
                cover_url: None,
            },
            status: f.status,
            // `currentchapter` is TEXT but holds the chapter count
            // (baseunits/uFavoritesManager.pas:352, :425).
            current_chapter: f.currentchapter.trim().parse().unwrap_or(0),
            enabled: f.enabled,
            date_added: f.dateadded.unwrap_or(0),
            date_last_checked: f.datelastchecked,
            date_last_updated: f.datelastupdated,
        };
        if !opts.dry_run {
            db.favorites().import(&favorite)?;
        }
        report.favorites.imported += 1;
    }
    Ok(())
}
