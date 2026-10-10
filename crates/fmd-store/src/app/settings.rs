//! Application settings: a key → JSON value table.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::db::Db;
use crate::error::Result;

pub struct SettingsRepo<'a> {
    db: &'a Db,
}

impl<'a> SettingsRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let json: Option<String> = {
            let conn = self.db.lock();
            conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?
        };
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    pub fn set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(value)?;
        let conn = self.db.lock();
        put(&conn, key, &json)
    }

    /// All or nothing.
    pub fn set_many<T: Serialize>(&self, entries: &[(&str, T)]) -> Result<()> {
        let entries = entries
            .iter()
            .map(|(key, value)| Ok((*key, serde_json::to_string(value)?)))
            .collect::<Result<Vec<_>>>()?;
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        for (key, json) in entries {
            put(&tx, key, &json)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove(&self, key: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }
}

pub(crate) fn put(conn: &Connection, key: &str, json: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, json],
    )?;
    Ok(())
}
