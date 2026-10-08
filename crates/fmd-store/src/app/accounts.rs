//! Module accounts, with username, password and cookies encrypted at rest. FMD2 keeps the same
//! fields in `modules.json`, encrypted with `EncryptString` (baseunits/WebsiteModules.pas:614-620,
//! :671-678).

use rusqlite::{OptionalExtension, params};

use crate::crypto::Cipher;
use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::sql::text_enum;

text_enum! {
    /// FMD2's `TAccountStatus` (baseunits/WebsiteModules.pas:78).
    pub enum AccountStatus {
        Unknown = "unknown",
        Checking = "checking",
        Valid = "valid",
        Invalid = "invalid",
    }
}

/// A module's account, in plaintext. Its `Debug` output leaves the credentials and cookies out,
/// so they never reach a log.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    pub module_id: String,
    pub enabled: bool,
    pub username: String,
    pub password: String,
    pub cookies: String,
    pub status: AccountStatus,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("module_id", &self.module_id)
            .field("enabled", &self.enabled)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// Repository for accounts. Obtain it with [`crate::AppDb::accounts`].
pub struct AccountRepo<'a> {
    db: &'a Db,
    cipher: &'a dyn Cipher,
}

impl<'a> AccountRepo<'a> {
    pub(crate) fn new(db: &'a Db, cipher: &'a dyn Cipher) -> Self {
        Self { db, cipher }
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<String> {
        String::from_utf8(self.cipher.decrypt(ciphertext)?)
            .map_err(|_| StoreError::Crypto("decrypted value is not UTF-8".into()))
    }

    pub fn get(&self, module_id: &str) -> Result<Option<Account>> {
        let row = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT enabled, username, password, cookies, status FROM accounts WHERE module_id = ?1",
                [module_id],
                |r| {
                    Ok((
                        r.get::<_, bool>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, AccountStatus>(4)?,
                    ))
                },
            )
            .optional()?
        };
        row.map(|(enabled, username, password, cookies, status)| {
            Ok(Account {
                module_id: module_id.to_string(),
                enabled,
                username: self.decrypt(&username)?,
                password: self.decrypt(&password)?,
                cookies: self.decrypt(&cookies)?,
                status,
            })
        })
        .transpose()
    }

    /// Inserts or replaces the module's account.
    pub fn upsert(&self, account: &Account) -> Result<()> {
        let username = self.cipher.encrypt(account.username.as_bytes())?;
        let password = self.cipher.encrypt(account.password.as_bytes())?;
        let cookies = self.cipher.encrypt(account.cookies.as_bytes())?;
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO accounts (module_id, enabled, username, password, cookies, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (module_id) DO UPDATE SET
                enabled = excluded.enabled, username = excluded.username,
                password = excluded.password, cookies = excluded.cookies, status = excluded.status",
            params![
                account.module_id,
                account.enabled,
                username,
                password,
                cookies,
                account.status
            ],
        )?;
        Ok(())
    }

    pub fn set_status(&self, module_id: &str, status: AccountStatus) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE accounts SET status = ?2 WHERE module_id = ?1",
            params![module_id, status],
        )?;
        Ok(())
    }

    pub fn delete(&self, module_id: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM accounts WHERE module_id = ?1", [module_id])?;
        Ok(())
    }
}
