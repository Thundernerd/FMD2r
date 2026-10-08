//! Events: the inbox and history shown in the UI.

use rusqlite::{OptionalExtension, Row, params};

use crate::app::tasks::TaskId;
use crate::db::Db;
use crate::error::Result;
use crate::sql::{now_ms, text_enum};

text_enum! {
    pub enum EventSeverity {
        Info = "info",
        Warning = "warning",
        Error = "error",
    }
}

/// Primary key of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EventId(pub i64);

/// Fields supplied when pushing an event.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    /// Free-form category, e.g. `module_update` or `new_chapters`.
    pub kind: String,
    pub severity: EventSeverity,
    pub module_id: Option<String>,
    pub task_id: Option<TaskId>,
    pub title: String,
    pub body: serde_json::Value,
}

/// A stored event.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: EventId,
    /// Unix milliseconds.
    pub ts: i64,
    pub kind: String,
    pub severity: EventSeverity,
    pub module_id: Option<String>,
    pub task_id: Option<TaskId>,
    pub title: String,
    pub body: serde_json::Value,
    pub read: bool,
}

/// Which events [`EventRepo::list`] returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventQuery {
    pub unread_only: bool,
    /// Maximum number of events; `None` for all.
    pub limit: Option<u32>,
}

const COLUMNS: &str = "id, ts, kind, severity, module_id, task_id, title, body, read";

/// Reads a row of [`COLUMNS`]; the body comes back as raw JSON text to be parsed outside rusqlite.
fn event_from_row(row: &Row<'_>) -> rusqlite::Result<(Event, String)> {
    Ok((
        Event {
            id: EventId(row.get(0)?),
            ts: row.get(1)?,
            kind: row.get(2)?,
            severity: row.get(3)?,
            module_id: row.get(4)?,
            task_id: row.get::<_, Option<i64>>(5)?.map(TaskId),
            title: row.get(6)?,
            body: serde_json::Value::Null,
            read: row.get(8)?,
        },
        row.get(7)?,
    ))
}

fn with_body((mut event, body): (Event, String)) -> Result<Event> {
    event.body = serde_json::from_str(&body)?;
    Ok(event)
}

/// Repository for events. Obtain it with [`crate::AppDb::events`].
pub struct EventRepo<'a> {
    db: &'a Db,
}

impl<'a> EventRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Stores an unread event stamped with the current time.
    pub fn push(&self, new: &NewEvent) -> Result<Event> {
        let body = serde_json::to_string(&new.body)?;
        let conn = self.db.lock();
        let row = conn.query_row(
            &format!(
                "INSERT INTO events (ts, kind, severity, module_id, task_id, title, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING {COLUMNS}"
            ),
            params![
                now_ms(),
                new.kind,
                new.severity,
                new.module_id,
                new.task_id.map(|t| t.0),
                new.title,
                body
            ],
            event_from_row,
        )?;
        with_body(row)
    }

    /// Events newest first.
    pub fn list(&self, query: &EventQuery) -> Result<Vec<Event>> {
        let rows: Vec<(Event, String)> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM events WHERE (?1 = 0 OR read = 0)
                 ORDER BY ts DESC, id DESC LIMIT ?2"
            ))?;
            let limit = query.limit.map_or(-1, i64::from);
            let rows = stmt.query_map(params![query.unread_only, limit], event_from_row)?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        rows.into_iter().map(with_body).collect()
    }

    /// The event with `id`, if any.
    pub fn get(&self, id: EventId) -> Result<Option<Event>> {
        let row = {
            let conn = self.db.lock();
            conn.query_row(
                &format!("SELECT {COLUMNS} FROM events WHERE id = ?1"),
                [id.0],
                event_from_row,
            )
            .optional()?
        };
        row.map(with_body).transpose()
    }

    /// Events stored after `after` (by id), oldest first; at most `limit` of them.
    pub fn list_after(&self, after: EventId, limit: u32) -> Result<Vec<Event>> {
        let rows: Vec<(Event, String)> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM events WHERE id > ?1 ORDER BY id LIMIT ?2"
            ))?;
            let rows = stmt.query_map(params![after.0, limit], event_from_row)?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        rows.into_iter().map(with_body).collect()
    }

    pub fn mark_read(&self, ids: &[EventId]) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE events SET read = 1 WHERE id = ?1")?;
            for id in ids {
                stmt.execute([id.0])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn mark_all_read(&self) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("UPDATE events SET read = 1 WHERE read = 0", [])?;
        Ok(())
    }

    pub fn unread_count(&self) -> Result<u64> {
        let conn = self.db.lock();
        Ok(
            conn.query_row("SELECT COUNT(*) FROM events WHERE read = 0", [], |r| {
                r.get(0)
            })?,
        )
    }
}
