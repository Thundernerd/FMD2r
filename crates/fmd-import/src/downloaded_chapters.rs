//! `downloadedchapters.db` → downloaded chapters.
//!
//! Schema: `TDownloadedChaptersDB.Create` (baseunits/DownloadedChaptersDB.pas:124-127): one row
//! per manga, `id` = `LowerCase(AModuleID+ALink)` (:53, :65, :74) and `chapters` the
//! newline-joined chapter links (`SetChapters`, :59-91). The `CleanStr` helper (:36-44) is never
//! called, so keys and chapter lists are stored as written.

use std::collections::HashMap;
use std::path::Path;

use fmd_store::AppDb;

use crate::ImportOptions;
use crate::error::ImportError;
use crate::fmd2::{lines, open_db, sqlite_error, text};
use crate::report::{ImportReport, SkipReason};

/// Length of FMD2's module ids: 32 hex digits (`m.ID` in lua/modules/*.lua).
const MODULE_ID_LEN: usize = 32;

/// What is known about module ids and links from the other sources, to split the lowercased
/// `module id + link` keys back into their parts.
#[derive(Default)]
pub(crate) struct KnownMangas {
    /// `LowerCase(module id + link)` → (module id, link) as FMD2 wrote them elsewhere.
    pairs: HashMap<String, (String, String)>,
    /// Module ids, e.g. from `modules.json`.
    module_ids: Vec<String>,
}

impl KnownMangas {
    pub(crate) fn add_manga(&mut self, module_id: String, link: String) {
        let key = format!("{module_id}{link}").to_lowercase();
        self.pairs.entry(key).or_insert((module_id, link));
    }

    pub(crate) fn add_module(&mut self, module_id: String) {
        if !module_id.is_empty() {
            self.module_ids.push(module_id);
        }
    }

    /// The (module id, link) the key was made of: a favorite's or task's own spelling when one
    /// matches, otherwise the longest known module id prefix, otherwise FMD2's usual 32 hex digit
    /// module id.
    fn split(&self, key: &str) -> Option<(String, String)> {
        if let Some(pair) = self.pairs.get(key) {
            return Some(pair.clone());
        }
        let known = self
            .module_ids
            .iter()
            .filter(|id| {
                key.len() > id.len()
                    && key.is_char_boundary(id.len())
                    && key[..id.len()].eq_ignore_ascii_case(id)
            })
            .max_by_key(|id| id.len());
        if let Some(id) = known {
            return Some((id.clone(), key[id.len()..].to_string()));
        }
        let (id, link) = key.split_at_checked(MODULE_ID_LEN)?;
        (id.bytes().all(|b| b.is_ascii_hexdigit()) && !link.is_empty())
            .then(|| (id.to_string(), link.to_string()))
    }
}

pub(crate) fn import(
    path: &Path,
    db: &AppDb,
    known: &KnownMangas,
    opts: &ImportOptions,
    report: &mut ImportReport,
) -> Result<(), ImportError> {
    let Some(conn) = open_db(path)? else {
        return Ok(());
    };
    report.downloaded_chapters.found = true;
    let rows = (|| {
        let mut stmt = conn.prepare(r#"SELECT "id", "chapters" FROM "downloadedchapters""#)?;
        let rows = stmt.query_map([], |r| Ok((text(r.get_ref(0)?), text(r.get_ref(1)?))))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })()
    .map_err(|e| sqlite_error(path, e))?;

    let repo = db.downloaded_chapters();
    for (key, chapters) in rows {
        let Some((module_id, link)) = known.split(&key) else {
            report.downloaded_chapters.skip(
                key,
                SkipReason::Invalid("cannot tell the module id from the link".into()),
            );
            continue;
        };
        let chapters: Vec<&str> = lines(&chapters)
            .into_iter()
            .filter(|c| !c.trim().is_empty())
            .collect();
        let mut new = Vec::new();
        for chapter in chapters {
            if !repo.contains(&module_id, &link, chapter)? {
                new.push(chapter);
            }
        }
        if new.is_empty() {
            report
                .downloaded_chapters
                .skip(key, SkipReason::AlreadyExists);
            continue;
        }
        if !opts.dry_run {
            repo.mark(&module_id, &link, &new)?;
        }
        report.downloaded_chapters.imported += 1;
    }
    Ok(())
}
