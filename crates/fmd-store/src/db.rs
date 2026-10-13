//! Connection setup and the forward-only migration runner shared by both databases.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::error::{Result, StoreError};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) const IN_MEMORY: &str = ":memory:";

/// One SQLite connection per file behind a mutex, locked per statement or transaction. SQLite
/// allows one writer anyway; WAL lets other processes read concurrently.
#[derive(Clone)]
pub(crate) struct Db {
    conn: Arc<Mutex<Connection>>,
    /// The file, or `None` for an in-memory database.
    file: Option<PathBuf>,
}

impl Db {
    /// Opens or creates the database and runs pending `migrations` (index `i` upgrades to
    /// version `i + 1`).
    pub(crate) fn open(path: &Path, name: &'static str, migrations: &[&str]) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&mut conn, name, migrations)?;
        let file = (path.as_os_str() != IN_MEMORY).then(|| path.to_owned());
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            file,
        })
    }

    /// Ignores poisoning: an unfinished `Transaction` rolls back on drop, so a panic leaves
    /// nothing half applied.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn schema_version(&self) -> Result<u32> {
        user_version(&self.lock())
    }

    /// Writes a compacted copy of the database to `dest` with `VACUUM INTO`, which reads one
    /// snapshot, so the copy is consistent while other connections write. A file database is read
    /// through a connection of its own, so the shared one stays free for writers meanwhile. Fails
    /// if `dest` exists and isn't empty.
    pub(crate) fn vacuum_into(&self, dest: &Path) -> Result<()> {
        let dest = dest.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} is not valid UTF-8", dest.display()),
            )
        })?;
        match &self.file {
            Some(file) => {
                let conn = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                conn.busy_timeout(BUSY_TIMEOUT)?;
                conn.execute("VACUUM INTO ?1", [dest])?;
            }
            None => {
                self.lock().execute("VACUUM INTO ?1", [dest])?;
            }
        }
        Ok(())
    }
}

fn user_version(conn: &Connection) -> Result<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Applies each pending migration in a transaction with its `user_version` bump. Forward-only:
/// a newer database is an error.
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
