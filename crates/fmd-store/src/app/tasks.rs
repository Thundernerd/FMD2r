//! Download tasks (`tasks`, `task_chapters`, `task_pages`), replacing FMD2's `downloads` table and
//! its newline-joined chapter and page columns (baseunits/DownloadsDB.pas:67-92).

use rusqlite::{OptionalExtension, Row, params};

use crate::db::Db;
use crate::error::Result;
use crate::sql::{next_sort_order, now_ms, reorder, text_enum};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub module_id: String,
    pub link: String,
    pub title: String,
    pub save_to: String,
    pub status: TaskStatus,
    pub enabled: bool,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChapter {
    pub link: String,
    pub name: String,
    pub custom_filename: Option<String>,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPage {
    pub url: String,
    pub container_url: String,
    pub filename: String,
    pub status: PageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPage {
    pub chapter_idx: u32,
    pub idx: u32,
    pub url: String,
    pub container_url: String,
    pub filename: String,
    pub status: PageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTask {
    pub task: NewTask,
    /// Unix milliseconds.
    pub date_added: i64,
    /// Unix milliseconds.
    pub date_last_downloaded: Option<i64>,
    pub current_chapter: u32,
    pub chapters: Vec<ImportedChapter>,
}

/// Its page count is the number of `pages`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedChapter {
    pub chapter: NewChapter,
    pub status: ChapterStatus,
    pub current_page: u32,
    pub pages: Vec<NewPage>,
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

pub struct TaskRepo<'a> {
    db: &'a Db,
}

impl<'a> TaskRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Appends to the queue.
    pub fn create(&self, new: &NewTask) -> Result<Task> {
        let conn = self.db.lock();
        let task = conn.query_row(
            &format!(
                "INSERT INTO tasks (module_id, link, title, save_to, status, enabled, sort_order, date_added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, {}, ?7)
                 RETURNING {TASK_COLUMNS}",
                next_sort_order("tasks")
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

    /// Appends to the queue with dates, progress, chapters and pages as given.
    pub fn import(&self, imported: &ImportedTask) -> Result<Task> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        let new = &imported.task;
        let task = tx.query_row(
            &format!(
                "INSERT INTO tasks (module_id, link, title, save_to, status, enabled, sort_order,
                    date_added, date_last_downloaded, current_chapter)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, {}, ?7, ?8, ?9)
                 RETURNING {TASK_COLUMNS}",
                next_sort_order("tasks")
            ),
            params![
                new.module_id,
                new.link,
                new.title,
                new.save_to,
                new.status,
                new.enabled,
                imported.date_added,
                imported.date_last_downloaded,
                imported.current_chapter
            ],
            task_from_row,
        )?;
        {
            let mut chapter_stmt = tx.prepare_cached(
                "INSERT INTO task_chapters
                    (task_id, idx, link, name, custom_filename, status, page_count, current_page)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            let mut page_stmt = tx.prepare_cached(
                "INSERT INTO task_pages (task_id, chapter_idx, idx, url, container_url, filename, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (idx, ch) in imported.chapters.iter().enumerate() {
                chapter_stmt.execute(params![
                    task.id.0,
                    idx,
                    ch.chapter.link,
                    ch.chapter.name,
                    ch.chapter.custom_filename,
                    ch.status,
                    ch.pages.len(),
                    ch.current_page
                ])?;
                for (page_idx, p) in ch.pages.iter().enumerate() {
                    page_stmt.execute(params![
                        task.id.0,
                        idx,
                        page_idx,
                        p.url,
                        p.container_url,
                        p.filename,
                        p.status
                    ])?;
                }
            }
        }
        tx.commit()?;
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

    /// In queue order.
    pub fn list(&self) -> Result<Vec<Task>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks ORDER BY sort_order"
        ))?;
        let rows = stmt.query_map([], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// In queue order.
    pub fn list_by_status(&self, status: TaskStatus) -> Result<Vec<Task>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE status = ?1 ORDER BY sort_order"
        ))?;
        let rows = stmt.query_map([status], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `None` clears the error.
    pub fn update_status(&self, id: TaskId, status: TaskStatus, error: Option<&str>) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE tasks SET status = ?2, error = ?3 WHERE id = ?1",
            params![id.0, status, error],
        )?;
        Ok(())
    }

    /// Replaces the chapters and drops their pages; each starts [`ChapterStatus::Pending`].
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

    pub fn update_chapter(
        &self,
        id: TaskId,
        chapter_idx: u32,
        status: ChapterStatus,
        current_page: u32,
    ) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE task_chapters SET status = ?3, current_page = ?4 WHERE task_id = ?1 AND idx = ?2",
            params![id.0, chapter_idx, status, current_page],
        )?;
        Ok(())
    }

    pub fn update_progress(
        &self,
        id: TaskId,
        current_chapter: u32,
        date_last_downloaded: Option<i64>,
    ) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE tasks SET current_chapter = ?2,
                date_last_downloaded = COALESCE(?3, date_last_downloaded)
             WHERE id = ?1",
            params![id.0, current_chapter, date_last_downloaded],
        )?;
        Ok(())
    }

    pub fn set_enabled(&self, id: TaskId, enabled: bool) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE tasks SET enabled = ?2 WHERE id = ?1",
            params![id.0, enabled],
        )?;
        Ok(())
    }

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

    /// Also sets the chapter's page count.
    pub fn set_pages(&self, id: TaskId, chapter_idx: u32, pages: &[NewPage]) -> Result<()> {
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
            for (idx, p) in pages.iter().enumerate() {
                stmt.execute(params![
                    id.0,
                    chapter_idx,
                    idx,
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

    /// Puts `ids` first; the rest keep their relative order.
    pub fn reorder(&self, ids: &[TaskId]) -> Result<()> {
        reorder(&mut self.db.lock(), "tasks", ids.iter().map(|id| id.0))
    }

    pub fn delete(&self, id: TaskId) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM tasks WHERE id = ?1", [id.0])?;
        Ok(())
    }
}
