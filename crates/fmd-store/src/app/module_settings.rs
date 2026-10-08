//! Per-module settings: what FMD2 keeps per module in `modules.json`
//! (baseunits/WebsiteModules.pas:545-696) and `TWebsiteModuleSettings`
//! (baseunits/WebsiteModulesSettings.pas:94-171). The JSON columns are opaque here; their shape
//! belongs to the settings model (T18).

use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Map, Value};

use crate::db::Db;
use crate::error::Result;

/// A module's stored settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleSettings {
    pub module_id: String,
    /// Whether the per-module overrides apply (FMD2's `Settings.Enabled`).
    pub enabled: bool,
    /// Values of the options declared with `AddOption*`, keyed by option name (`Options` in
    /// `modules.json`).
    pub options: Value,
    /// HTTP overrides: user agent, cookies, proxy (`Settings.HTTP` in `modules.json`).
    pub http: Value,
    /// Limit overrides such as the connection limit.
    pub limits: Value,
    /// Serialised cookie jar (FMD2's per-module `Cookies` array).
    pub cookie_jar: Option<Vec<u8>>,
}

impl ModuleSettings {
    /// Settings for a module that has none stored yet.
    pub fn new(module_id: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            enabled: false,
            options: Value::Object(Map::new()),
            http: Value::Object(Map::new()),
            limits: Value::Object(Map::new()),
            cookie_jar: None,
        }
    }
}

/// Columns as read from the database, before the JSON columns are parsed.
struct RawSettings {
    enabled: bool,
    options: String,
    http: String,
    limits: String,
    cookie_jar: Option<Vec<u8>>,
}

fn raw_from_row(row: &Row<'_>) -> rusqlite::Result<RawSettings> {
    Ok(RawSettings {
        enabled: row.get(0)?,
        options: row.get(1)?,
        http: row.get(2)?,
        limits: row.get(3)?,
        cookie_jar: row.get(4)?,
    })
}

const SELECT: &str =
    "SELECT enabled, options, http, limits, cookie_jar FROM module_settings WHERE module_id = ?1";

/// Repository for per-module settings. Obtain it with [`crate::AppDb::module_settings`].
///
/// `option`/`set_option` and `cookie_jar`/`set_cookie_jar` are the operations behind fmd-lua's
/// `ModuleSettingsStore` (T06).
pub struct ModuleSettingsRepo<'a> {
    db: &'a Db,
}

impl<'a> ModuleSettingsRepo<'a> {
    pub(crate) fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub fn get(&self, module_id: &str) -> Result<Option<ModuleSettings>> {
        let raw = {
            let conn = self.db.lock();
            conn.query_row(SELECT, [module_id], raw_from_row)
                .optional()?
        };
        raw.map(|raw| {
            Ok(ModuleSettings {
                module_id: module_id.to_string(),
                enabled: raw.enabled,
                options: serde_json::from_str(&raw.options)?,
                http: serde_json::from_str(&raw.http)?,
                limits: serde_json::from_str(&raw.limits)?,
                cookie_jar: raw.cookie_jar,
            })
        })
        .transpose()
    }

    /// Inserts or replaces all of the module's settings.
    pub fn upsert(&self, settings: &ModuleSettings) -> Result<()> {
        let options = serde_json::to_string(&settings.options)?;
        let http = serde_json::to_string(&settings.http)?;
        let limits = serde_json::to_string(&settings.limits)?;
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO module_settings (module_id, enabled, options, http, limits, cookie_jar)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (module_id) DO UPDATE SET
                enabled = excluded.enabled, options = excluded.options, http = excluded.http,
                limits = excluded.limits, cookie_jar = excluded.cookie_jar",
            params![
                settings.module_id,
                settings.enabled,
                options,
                http,
                limits,
                settings.cookie_jar
            ],
        )?;
        Ok(())
    }

    /// The stored value of option `name`, or `None` when it was never set.
    pub fn option(&self, module_id: &str, name: &str) -> Result<Option<Value>> {
        let options: Option<String> = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT options FROM module_settings WHERE module_id = ?1",
                [module_id],
                |r| r.get(0),
            )
            .optional()?
        };
        let Some(options) = options else {
            return Ok(None);
        };
        let mut options: Value = serde_json::from_str(&options)?;
        Ok(options.get_mut(name).map(Value::take))
    }

    /// Stores option `name`, keeping the module's other options.
    pub fn set_option(&self, module_id: &str, name: &str, value: &Value) -> Result<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT options FROM module_settings WHERE module_id = ?1",
                [module_id],
                |r| r.get(0),
            )
            .optional()?;
        let mut options = match stored {
            Some(json) => match serde_json::from_str(&json)? {
                Value::Object(map) => map,
                _ => Map::new(),
            },
            None => Map::new(),
        };
        options.insert(name.to_string(), value.clone());
        tx.execute(
            "INSERT INTO module_settings (module_id, options) VALUES (?1, ?2)
             ON CONFLICT (module_id) DO UPDATE SET options = excluded.options",
            params![module_id, serde_json::to_string(&options)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn cookie_jar(&self, module_id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.db.lock();
        Ok(conn
            .query_row(
                "SELECT cookie_jar FROM module_settings WHERE module_id = ?1",
                [module_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Stores (or with `None` clears) the module's serialised cookie jar.
    pub fn set_cookie_jar(&self, module_id: &str, jar: Option<&[u8]>) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO module_settings (module_id, cookie_jar) VALUES (?1, ?2)
             ON CONFLICT (module_id) DO UPDATE SET cookie_jar = excluded.cookie_jar",
            params![module_id, jar],
        )?;
        Ok(())
    }
}
