//! Connection setup and the forward-only migration runner shared by both databases.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{Result, StoreError};

/// How long a writer waits on a lock held by another connection before failing with `SQLITE_BUSY`.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// One SQLite connection shared behind a mutex.
///
/// Every repository call locks the mutex for the duration of one statement or one transaction, so
/// a `Db` can be cloned freely and used from worker threads directly, or from async code inside
/// `tokio::task::spawn_blocking`. A single connection per file keeps writes serialised (SQLite only
/// allows one writer anyway); WAL mode lets other processes (e.g. `sqlite3` while debugging) read
/// concurrently.
#[derive(Clone)]
pub(crate) struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (creating if needed) the database at `path`, applies the connection pragmas and runs
    /// every pending migration in `migrations` (index `i` upgrades the schema to version `i + 1`).
    pub(crate) fn open(path: &Path, name: &'static str, migrations: &[&str]) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&mut conn, name, migrations)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Locks the connection. A panic while the lock was held cannot leave a transaction half
    /// applied (an unfinished `Transaction` rolls back on drop), so poisoning is ignored.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn schema_version(&self) -> Result<u32> {
        user_version(&self.lock())
    }
}

fn user_version(conn: &Connection) -> Result<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Applies migrations `user_version..migrations.len()`, each in its own transaction together with
/// the `user_version` bump. Migrations are forward-only: a database newer than this build is an
/// error rather than something to downgrade.
fn migrate(conn: &mut Connection, name: &'static str, migrations: &[&str]) -> Result<()> {
    let supported = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
    let found = user_version(conn)?;
    if found > supported {
        return Err(StoreError::SchemaTooNew {
            db: name,
            found,
            supported,
        });
    }
    for (version, sql) in (found..supported).zip(migrations.iter().skip(found as usize)) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version + 1)?;
        tx.commit()?;
    }
    Ok(())
}
