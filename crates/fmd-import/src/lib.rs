//! FMD2 `userdata` importer into an FMD2r `app.db` (file names from
//! baseunits/FMDOptions.pas:288-295). Idempotent: rows already in the store are skipped and
//! reported. Downloaded files are not copied.

mod downloaded_chapters;
mod downloads;
mod error;
mod favorites;
mod fmd2;
mod modules;
mod paths;
mod report;
mod settings;
mod timezone;

use std::path::Path;

use fmd_core::settings::SettingsService;
use fmd_store::{AppDb, Cipher};

use downloaded_chapters::KnownMangas;

pub use error::ImportError;
pub use paths::{PathMap, PathMapParseError};
pub use report::{ImportReport, SkipReason, Skipped, SourceReport, Unmapped};
pub use timezone::{TimeZone, UnknownTimeZone};

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
    /// The zone FMD2 ran in, which its timestamps are local time of; this machine's by default.
    pub timezone: TimeZone,
}

/// Account credentials are decrypted with FMD2's `DecryptString` and stored encrypted with
/// `cipher`. A missing source file is reported as not found; an unreadable one stops the import
/// (sources imported before it stay imported).
pub fn import(
    userdata: &Path,
    db: &AppDb,
    cipher: &dyn Cipher,
    opts: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let settings = SettingsService::load(db.clone())?;
    import_into(userdata, db, cipher, &settings, opts, |_| {})
}

/// How far [`import_into`] is: `done` of `total` sources read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportProgress {
    pub done: u64,
    pub total: u64,
}

/// [`import`] into a running app: settings go through the app's live `settings` service, and
/// `progress` is called after each source.
pub fn import_into(
    userdata: &Path,
    db: &AppDb,
    cipher: &dyn Cipher,
    settings: &SettingsService,
    opts: &ImportOptions,
    mut progress: impl FnMut(ImportProgress),
) -> Result<ImportReport, ImportError> {
    const SOURCES: u64 = 5;
    let mut step = |done| {
        progress(ImportProgress {
            done,
            total: SOURCES,
        })
    };
    step(0);
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
    step(1);
    // Before favorites, which merge their own downloaded lists into the same table.
    downloaded_chapters::import(
        &userdata.join("downloadedchapters.db"),
        db,
        &known,
        opts,
        &mut report,
    )?;
    step(2);
    favorites::import(&favorites_db, db, opts, &mut report)?;
    step(3);
    let folders = modules::import(modules.as_deref(), db, cipher, opts, &mut report)?;
    step(4);
    settings::import(&userdata.join("settings.json"), settings, opts, &mut report)?;
    // After the settings, so a website folder that is the download folder is the default.
    modules::add_destinations(settings, &folders, opts)?;
    step(SOURCES);
    Ok(report)
}
