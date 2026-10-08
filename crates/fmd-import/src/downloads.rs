//! `downloads.db` → tasks.
//!
//! Schema: `TDownloadsDB.Create` (baseunits/DownloadsDB.pas:67-89); rows are read in `"order"`
//! like `TDownloadManager.Restore` (:91, baseunits/uDownloadsManager.pas:1638-1692).
//!
//! Not imported, because FMD2 derives them from the other columns: `status` and `progress` are
//! display text (rewritten from the task state, e.g. :1872, :1883), and `numberofpages` is the
//! length of `pagelinks` (:1213, :1253).

use std::collections::HashSet;
use std::path::Path;

use fmd_store::{
    AppDb, ChapterStatus, ImportedChapter, ImportedTask, NewChapter, NewPage, NewTask, PageStatus,
    TaskStatus,
};
use rusqlite::Row;

use crate::ImportOptions;
use crate::error::ImportError;
use crate::fmd2::{datetime, lines, open_db, sql_bool, sql_int, sql_text, sqlite_error};
use crate::paths::translate;
use crate::report::{ImportReport, SkipReason};

/// One `downloads` row, decoded.
struct Download {
    enabled: bool,
    taskstatus: i64,
    chapterptr: i64,
    currentpage: i64,
    moduleid: String,
    link: String,
    title: String,
    saveto: String,
    dateadded: Option<i64>,
    datelastdownloaded: Option<i64>,
    chapterslinks: String,
    chaptersnames: String,
    pagelinks: String,
    pagecontainerlinks: String,
    filenames: String,
    customfilenames: String,
    chaptersstatus: String,
}

impl Download {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            enabled: sql_bool(row.get_ref("enabled")?),
            taskstatus: sql_int(row.get_ref("taskstatus")?),
            chapterptr: sql_int(row.get_ref("chapterptr")?),
            currentpage: sql_int(row.get_ref("currentpage")?),
            moduleid: sql_text(row.get_ref("moduleid")?),
            link: sql_text(row.get_ref("link")?),
            title: sql_text(row.get_ref("title")?),
            saveto: sql_text(row.get_ref("saveto")?),
            dateadded: datetime(row.get_ref("dateadded")?),
            datelastdownloaded: datetime(row.get_ref("datelastdownloaded")?),
            chapterslinks: sql_text(row.get_ref("chapterslinks")?),
            chaptersnames: sql_text(row.get_ref("chaptersnames")?),
            pagelinks: sql_text(row.get_ref("pagelinks")?),
            pagecontainerlinks: sql_text(row.get_ref("pagecontainerlinks")?),
            filenames: sql_text(row.get_ref("filenames")?),
            customfilenames: sql_text(row.get_ref("customfilenames")?),
            chaptersstatus: sql_text(row.get_ref("chaptersstatus")?),
        })
    }
}

/// The task statuses of FMD2's `TDownloadStatusType`, whose ordinal is the stored `taskstatus`
/// (baseunits/uDownloadsManager.pas:19-31, written as `Integer(Status)` at :1451).
fn task_status(taskstatus: i64, resume_in_progress: bool) -> TaskStatus {
    let in_progress = if resume_in_progress {
        TaskStatus::Waiting
    } else {
        TaskStatus::Stopped
    };
    match taskstatus {
        // STATUS_STOP
        0 => TaskStatus::Stopped,
        // STATUS_WAIT, STATUS_PREPARE, STATUS_DOWNLOAD: FMD2 restarts these at startup
        // (baseunits/uDownloadsManager.pas:1868-1884).
        1..=3 => in_progress,
        // STATUS_FINISH
        4 => TaskStatus::Finished,
        // STATUS_CONVERT, STATUS_COMPRESS
        5 | 6 => in_progress,
        // STATUS_PROBLEM (finished with errors) and STATUS_FAILED
        7 | 8 => TaskStatus::Failed,
        // STATUS_NONE and anything unknown.
        _ => TaskStatus::Stopped,
    }
}

/// `ChaptersStatus` markers: 'D' downloaded, 'F' failed, 'P' pending
/// (baseunits/uDownloadsManager.pas:1314-1318).
fn chapter_status(marker: &str) -> ChapterStatus {
    match marker.trim() {
        "D" => ChapterStatus::Downloaded,
        "F" => ChapterStatus::Failed,
        _ => ChapterStatus::Pending,
    }
}

/// A page of the current chapter. `PageLinks` holds the image URL, or 'W' while it waits for
/// one and 'D' once it is saved (baseunits/uDownloadsManager.pas:350-360, :407).
fn page(link: &str, container_url: &str, filename: &str) -> NewPage {
    let (url, status) = match link.trim() {
        "D" => (String::new(), PageStatus::Downloaded),
        "W" => (String::new(), PageStatus::Waiting),
        _ => (link.to_string(), PageStatus::Waiting),
    };
    NewPage {
        url,
        container_url: container_url.to_string(),
        filename: filename.to_string(),
        status,
    }
}

fn to_task(d: &Download, opts: &ImportOptions, report: &mut ImportReport) -> ImportedTask {
    let links = lines(&d.chapterslinks);
    let names = lines(&d.chaptersnames);
    let mut markers: Vec<&str> = lines(&d.chaptersstatus);
    // Missing markers: chapters before the current one count as downloaded, the rest as pending,
    // as `TTaskThread.Execute` fills them in (baseunits/uDownloadsManager.pas:1117-1124).
    while (markers.len() as i64) < d.chapterptr - 1 {
        markers.push("D");
    }
    let current = usize::try_from(d.chapterptr).unwrap_or(0);
    // `customfilenames` is the task's single filename template despite its name
    // (`CustomFileName`, baseunits/uDownloadsManager.pas:1467, :1679).
    let custom_filename =
        Some(d.customfilenames.clone()).filter(|template| !template.trim().is_empty());

    let chapters = links
        .iter()
        .enumerate()
        .map(|(i, link)| {
            let pages = if i == current {
                let containers = lines(&d.pagecontainerlinks);
                let filenames = lines(&d.filenames);
                lines(&d.pagelinks)
                    .iter()
                    .enumerate()
                    .map(|(p, url)| {
                        page(
                            url,
                            containers.get(p).copied().unwrap_or(""),
                            filenames.get(p).copied().unwrap_or(""),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            ImportedChapter {
                chapter: NewChapter {
                    link: link.to_string(),
                    name: names.get(i).copied().unwrap_or("").to_string(),
                    custom_filename: custom_filename.clone(),
                },
                status: chapter_status(markers.get(i).copied().unwrap_or("P")),
                current_page: if i == current {
                    u32::try_from(d.currentpage).unwrap_or(0)
                } else {
                    0
                },
                pages,
            }
        })
        .collect();

    ImportedTask {
        task: NewTask {
            module_id: d.moduleid.clone(),
            link: d.link.clone(),
            title: d.title.clone(),
            save_to: translate(&opts.path_maps, &d.saveto, report),
            status: task_status(d.taskstatus, opts.resume_in_progress),
            enabled: d.enabled,
        },
        date_added: d.dateadded.unwrap_or(0),
        date_last_downloaded: d.datelastdownloaded,
        current_chapter: u32::try_from(d.chapterptr).unwrap_or(0),
        chapters,
    }
}

fn read(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<Download>> {
    let mut stmt = conn.prepare(r#"SELECT * FROM "downloads" ORDER BY "order""#)?;
    let rows = stmt.query_map([], Download::from_row)?;
    rows.collect()
}

/// The (module id, link) of every task in `downloads.db`.
pub(crate) fn keys(path: &Path) -> Result<Vec<(String, String)>, ImportError> {
    let Some(conn) = open_db(path)? else {
        return Ok(Vec::new());
    };
    read(&conn)
        .map(|rows| rows.into_iter().map(|d| (d.moduleid, d.link)).collect())
        .map_err(|e| sqlite_error(path, e))
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
    report.tasks.found = true;
    let downloads = read(&conn).map_err(|e| sqlite_error(path, e))?;

    // A task is the same task when module, link and the time it was added match.
    let mut existing: HashSet<(String, String, i64)> = db
        .tasks()
        .list()?
        .into_iter()
        .map(|t| (t.module_id, t.link, t.date_added))
        .collect();
    for d in &downloads {
        let item = format!("{} {}", d.moduleid, d.link);
        if d.moduleid.is_empty() || d.link.is_empty() {
            report
                .tasks
                .skip(item, SkipReason::Invalid("no module id or link".into()));
            continue;
        }
        let task = to_task(d, opts, report);
        let key = (
            task.task.module_id.clone(),
            task.task.link.clone(),
            task.date_added,
        );
        if !existing.insert(key) {
            report.tasks.skip(item, SkipReason::AlreadyExists);
            continue;
        }
        if !opts.dry_run {
            db.tasks().import(&task)?;
        }
        report.tasks.imported += 1;
    }
    Ok(())
}
