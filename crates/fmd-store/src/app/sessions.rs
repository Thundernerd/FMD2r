//! Web UI login sessions (T41). No FMD2 counterpart: FMD2 has no web server.

use rusqlite::params;

use crate::db::Db;
use crate::error::Result;

/// Repository for login sessions, keyed by a hash of the session cookie. Obtain it with
/// [`crate::AppDb::sessions`]. Timestamps are Unix milliseconds.
pub struct SessionRepo<'a> {
    db: &'a Db,
}

impl<'a> SessionRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Records a session created (and last seen) at `now`.
    pub fn create(&self, token_hash: &[u8], now: i64) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO sessions (token_hash, created_at, last_seen) VALUES (?1, ?2, ?2)",
            params![token_hash, now],
        )?;
        Ok(())
    }

    /// Marks the session seen at `now` if it exists, was last seen at or after `seen_since` and
    /// was created at or after `created_since`; returns whether it did.
    pub fn renew(
        &self,
        token_hash: &[u8],
        now: i64,
        seen_since: i64,
        created_since: i64,
    ) -> Result<bool> {
        let conn = self.db.lock();
        let renewed = conn.execute(
            "UPDATE sessions SET last_seen = max(last_seen, ?2)
             WHERE token_hash = ?1 AND last_seen >= ?3 AND created_at >= ?4",
            params![token_hash, now, seen_since, created_since],
        )?;
        Ok(renewed == 1)
    }

    /// Ends one session; ending an unknown one is not an error.
    pub fn delete(&self, token_hash: &[u8]) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
        Ok(())
    }

    /// Ends every session.
    pub fn delete_all(&self) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM sessions", [])?;
        Ok(())
    }

    /// Drops the sessions last seen before `seen_since` or created before `created_since`.
    pub fn delete_expired(&self, seen_since: i64, created_since: i64) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "DELETE FROM sessions WHERE last_seen < ?1 OR created_at < ?2",
            params![seen_since, created_since],
        )?;
        Ok(())
    }
}
