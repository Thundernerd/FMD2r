//! Each module's manga list in `lists.db`, built by [`ListUpdater`] or imported by
//! [`DbImporter`], run as [`ListJobs`].

mod import;
mod info;
mod jobs;
mod updater;

use std::time::{SystemTime, UNIX_EPOCH};

pub use import::{DbImporter, ImportError, db_url};
pub use jobs::{
    ListEvent, ListEventKind, ListFailureReason, ListJobError, ListJobKind, ListJobs, ListModules,
};
pub use updater::{ListError, ListPhase, ListProgress, ListUpdater, UpdateOptions, UpdateOutcome};

/// `DateToJDN(Now)` (baseunits/uBaseUnit.pas:2740-2751), for the UTC date where FMD2 uses the
/// local one.
pub fn today_jdn() -> i64 {
    /// `DateToJDN` of 1970-01-01.
    const UNIX_EPOCH_JDN: i64 = 2_440_588;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    UNIX_EPOCH_JDN + i64::try_from(secs / 86_400).unwrap_or(0)
}
