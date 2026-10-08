//! FMD2 userdata importer (`downloads.db`, `favorites.db`, `downloadedchapters.db`, `modules.json`,
//! `settings.json`).
//!
//! [`import`] reads an FMD2 `userdata` directory (file names from baseunits/FMDOptions.pas:288-295)
//! and writes its state into an FMD2r `app.db`. It is idempotent: rows already in the store are
//! skipped and listed in the [`ImportReport`]. With [`ImportOptions::dry_run`] nothing is written
//! and the report says what would be imported. Downloaded files are not copied.
//!
//! FMD2 stores dates as local time without a zone; they are imported as if they were UTC.

mod downloaded_chapters;
mod downloads;
mod error;
mod favorites;
mod fmd2;
mod modules;
mod paths;
mod report;
mod settings;

use std::path::Path;

use fmd_store::{AppDb, Cipher};

use downloaded_chapters::KnownMangas;

pub use error::ImportError;
pub use paths::{PathMap, PathMapParseError};
pub use report::{ImportReport, SkipReason, Skipped, SourceReport, Unmapped};

/// How [`import`] runs.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Read and check everything, but write nothing.
    pub dry_run: bool,
    /// Import tasks FMD2 was running or about to run (waiting, preparing, downloading, converting,
    /// compressing) as waiting, so they resume, instead of stopped.
    pub resume_in_progress: bool,
    /// Rewrites save-to paths, e.g. FMD2's Windows paths to a Linux root.
    pub path_maps: Vec<PathMap>,
}

/// Imports the FMD2 `userdata` directory at `userdata` into `db`. Account credentials are
/// decrypted with FMD2's `DecryptString` and stored encrypted with `cipher`.
///
/// A missing source file is reported as not found; an unreadable one stops the import with an
/// error (sources imported before it stay imported).
pub fn import(
    userdata: &Path,
    db: &AppDb,
    cipher: &dyn Cipher,
    opts: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut report = ImportReport {
        dry_run: opts.dry_run,
        ..ImportReport::default()
    };
    let downloads_db = userdata.join("downloads.db");
    let favorites_db = userdata.join("favorites.db");

    let modules = modules::read(&userdata.join("modules.json"))?;

    let mut known = KnownMangas::default();
    for module_id in modules.iter().flat_map(|m| modules::module_ids(m)) {
        known.add_module(module_id);
    }
    for (module_id, link) in favorites::keys(&favorites_db)?
        .into_iter()
        .chain(downloads::keys(&downloads_db)?)
    {
        known.add_manga(module_id, link);
    }

    downloads::import(&downloads_db, db, opts, &mut report)?;
    // Before favorites, which merge their own downloaded lists into the same table.
    downloaded_chapters::import(
        &userdata.join("downloadedchapters.db"),
        db,
        &known,
        opts,
        &mut report,
    )?;
    favorites::import(&favorites_db, db, opts, &mut report)?;
    modules::import(modules.as_deref(), db, cipher, opts, &mut report)?;
    settings::import(&userdata.join("settings.json"), db, opts, &mut report)?;
    Ok(report)
}
