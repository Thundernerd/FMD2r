//! Download tasks (`tasks`, `task_chapters`, `task_pages`), replacing FMD2's `downloads` table and
//! its newline-joined chapter and page columns (baseunits/DownloadsDB.pas:67-92).

use rusqlite::{OptionalExtension, Row, params};

use crate::db::Db;
use crate::error::Result;
use crate::sql::{now_ms, text_enum};

text_enum! {
    /// Task status, mirroring FMD2's `TDownloadStatusType` (baseunits/uDownloadsManager.pas:19-31)
    /// minus its internal `STATUS_PROBLEM`/`STATUS_NONE`, plus `Disabled`.
    pub enum TaskStatus {
        Stopped = "stopped",
        Waiting = "waiting",
        Preparing = "preparing",
        Downloading = "downloading",
        Converting = "converting",
        Compressing = "compressing",
        Finished = "finished",
        Failed = "failed",
        Disabled = "disabled",
    }
}

text_enum! {
    /// Per-chapter status, FMD2's `ChaptersStatus` markers `P`/`D`/`F`
    /// (baseunits/uDownloadsManager.pas:1117-1123, :1314-1318).
    pub enum ChapterStatus {
        Pending = "pending",
        Downloaded = "downloaded",
        Failed = "failed",
    }
}

text_enum! {
    /// Per-page status, FMD2's `PageLinks` markers `W`/`D` (baseunits/uDownloadsManager.pas:353, :407).
    pub enum PageStatus {
        Waiting = "waiting",
        Downloaded = "downloaded",
    }
}

/// Primary key of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub i64);

/// Fields supplied when creating a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub module_id: String,
    pub link: String,
    pub title: String,
    pub save_to: String,
    pub status: TaskStatus,
    pub enabled: bool,
}

/// A stored task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub module_id: String,
    pub link: String,
    pub title: String,
    pub save_to: String,
    pub status: TaskStatus,
    pub enabled: bool,
    pub sort_order: i64,
    /// Unix milliseconds.
    pub date_added: i64,
    /// Unix milliseconds.
    pub date_last_downloaded: Option<i64>,
    /// Index of the chapter being downloaded (FMD2's `chapterptr`).
    pub current_chapter: u32,
    pub error: Option<String>,
}

/// Fields supplied when setting a task's chapter list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChapter {
    pub link: String,
    pub name: String,
    pub custom_filename: Option<String>,
}

/// A stored chapter of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskChapter {
    pub idx: u32,
    pub link: String,
    pub name: String,
    pub custom_filename: Option<String>,
    pub status: ChapterStatus,
    pub page_count: u32,
    pub current_page: u32,
}

/// A page of a task's chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPage {
    pub chapter_idx: u32,
    pub idx: u32,
    pub url: String,
    pub container_url: String,
    pub filename: String,
    pub status: PageStatus,
}

const TASK_COLUMNS: &str = "id, module_id, link, title, save_to, status, enabled, sort_order, \
     date_added, date_last_downloaded, current_chapter, error";

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: TaskId(row.get(0)?),
        module_id: row.get(1)?,
        link: row.get(2)?,
        title: row.get(3)?,
        save_to: row.get(4)?,
        status: row.get(5)?,
        enabled: row.get(6)?,
        sort_order: row.get(7)?,
        date_added: row.get(8)?,
        date_last_downloaded: row.get(9)?,
        current_chapter: row.get(10)?,
        error: row.get(11)?,
    })
}

/// Repository for download tasks. Obtain it with [`crate::AppDb::tasks`].
pub struct TaskRepo<'a> {
    db: &'a Db,
}

impl<'a> TaskRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Inserts a task at the end of the queue, stamped with the current time.
    pub fn create(&self, new: &NewTask) -> Result<Task> {
        let conn = self.db.lock();
        let task = conn.query_row(
            &format!(
                "INSERT INTO tasks (module_id, link, title, save_to, status, enabled, sort_order, date_added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM tasks), ?7)
                 RETURNING {TASK_COLUMNS}"
            ),
            params![
                new.module_id,
                new.link,
                new.title,
                new.save_to,
                new.status,
                new.enabled,
                now_ms()
            ],
            task_from_row,
        )?;
        Ok(task)
    }

    pub fn get(&self, id: TaskId) -> Result<Option<Task>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
                [id.0],
                task_from_row,
            )
            .optional()?)
    }

    /// Every task in queue order.
    pub fn list(&self) -> Result<Vec<Task>> {
        let conn = self.db.lock();
        let mut stmt =
            conn.prepare_cached(&format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY sort_order"))?;
        let rows = stmt.query_map([], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Tasks with `status`, in queue order.
    pub fn list_by_status(&self, status: TaskStatus) -> Result<Vec<Task>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE status = ?1 ORDER BY sort_order"
        ))?;
        let rows = stmt.query_map([status], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Sets a task's status and its error text (`None` clears it).
    pub fn update_status(&self, id: TaskId, status: TaskStatus, error: Option<&str>) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE tasks SET status = ?2, error = ?3 WHERE id = ?1",
            params![id.0, status, error],
        )?;
        Ok(())
    }

    /// Replaces the task's chapters (and drops their pages). Chapter `i` gets index `i` and
    /// status [`ChapterStatus::Pending`].
    pub fn set_chapters(&self, id: TaskId, chapters: &[NewChapter]) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM task_chapters WHERE task_id = ?1", [id.0])?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO task_chapters (task_id, idx, link, name, custom_filename, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (idx, ch) in chapters.iter().enumerate() {
                stmt.execute(params![
                    id.0,
                    idx,
                    ch.link,
                    ch.name,
                    ch.custom_filename,
                    ChapterStatus::Pending
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The task's chapters in index order.
    pub fn chapters(&self, id: TaskId) -> Result<Vec<TaskChapter>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT idx, link, name, custom_filename, status, page_count, current_page
             FROM task_chapters WHERE task_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_map([id.0], |row| {
            Ok(TaskChapter {
                idx: row.get(0)?,
                link: row.get(1)?,
                name: row.get(2)?,
                custom_filename: row.get(3)?,
                status: row.get(4)?,
                page_count: row.get(5)?,
                current_page: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replaces the pages of chapter `chapter_idx` and sets its page count. Each page's own
    /// `chapter_idx` is ignored in favour of the argument.
    pub fn set_pages(&self, id: TaskId, chapter_idx: u32, pages: &[TaskPage]) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM task_pages WHERE task_id = ?1 AND chapter_idx = ?2",
            params![id.0, chapter_idx],
        )?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO task_pages (task_id, chapter_idx, idx, url, container_url, filename, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for p in pages {
                stmt.execute(params![
                    id.0,
                    chapter_idx,
                    p.idx,
                    p.url,
                    p.container_url,
                    p.filename,
                    p.status
                ])?;
            }
        }
        tx.execute(
            "UPDATE task_chapters SET page_count = ?3 WHERE task_id = ?1 AND idx = ?2",
            params![id.0, chapter_idx, pages.len()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The pages of chapter `chapter_idx` in index order.
    pub fn pages(&self, id: TaskId, chapter_idx: u32) -> Result<Vec<TaskPage>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT chapter_idx, idx, url, container_url, filename, status
             FROM task_pages WHERE task_id = ?1 AND chapter_idx = ?2 ORDER BY idx",
        )?;
        let rows = stmt.query_map(params![id.0, chapter_idx], |row| {
            Ok(TaskPage {
                chapter_idx: row.get(0)?,
                idx: row.get(1)?,
                url: row.get(2)?,
                container_url: row.get(3)?,
                filename: row.get(4)?,
                status: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Overwrites the stored page identified by (`page.chapter_idx`, `page.idx`).
    pub fn update_page(&self, id: TaskId, page: &TaskPage) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE task_pages SET url = ?4, container_url = ?5, filename = ?6, status = ?7
             WHERE task_id = ?1 AND chapter_idx = ?2 AND idx = ?3",
            params![
                id.0,
                page.chapter_idx,
                page.idx,
                page.url,
                page.container_url,
                page.filename,
                page.status
            ],
        )?;
        Ok(())
    }

    /// Puts the given tasks first, in the given order; tasks not listed keep their relative order
    /// after them.
    pub fn reorder(&self, ids: &[TaskId]) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        let rest: Vec<i64> = {
            let mut stmt = tx.prepare_cached("SELECT id FROM tasks ORDER BY sort_order")?;
            let all = stmt.query_map([], |r| r.get(0))?;
            all.filter(|id| !matches!(id, Ok(id) if ids.contains(&TaskId(*id))))
                .collect::<rusqlite::Result<_>>()?
        };
        {
            let mut stmt = tx.prepare_cached("UPDATE tasks SET sort_order = ?2 WHERE id = ?1")?;
            for (order, id) in ids.iter().map(|id| id.0).chain(rest).enumerate() {
                stmt.execute(params![id, order])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes the task with its chapters and pages.
    pub fn delete(&self, id: TaskId) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM tasks WHERE id = ?1", [id.0])?;
        Ok(())
    }
}
