//! What an import did.

use std::fmt;

use serde::Serialize;

/// The outcome of [`crate::import`], per source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// Nothing was written; the counts say what would have been imported.
    pub dry_run: bool,
    /// `downloads.db` → tasks.
    pub tasks: SourceReport,
    /// `favorites.db` → favorites.
    pub favorites: SourceReport,
    /// `downloadedchapters.db` → downloaded chapters, counted per manga.
    pub downloaded_chapters: SourceReport,
    /// `modules.json` → per-module settings, options and cookies.
    pub module_settings: SourceReport,
    /// `modules.json` → accounts.
    pub accounts: SourceReport,
    /// `settings.json` → application settings, counted per key.
    pub settings: SourceReport,
    /// FMD2 data FMD2r has no place for.
    pub unmapped: Vec<Unmapped>,
    /// Things that were imported but may need attention, such as Windows paths no
    /// [`crate::PathMap`] rewrote.
    pub warnings: Vec<String>,
}

/// What one source contributed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SourceReport {
    /// The source file exists in the userdata directory.
    pub found: bool,
    pub imported: usize,
    pub skipped: Vec<Skipped>,
}

impl SourceReport {
    pub(crate) fn skip(&mut self, item: impl Into<String>, reason: SkipReason) {
        self.skipped.push(Skipped {
            item: item.into(),
            reason,
        });
    }

    /// How many items were skipped because the store already had them.
    pub fn already_existing(&self) -> usize {
        self.skipped
            .iter()
            .filter(|s| s.reason == SkipReason::AlreadyExists)
            .count()
    }
}

/// An item that was not imported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    /// Identifies the item, e.g. `<module id> <link>` or a settings key.
    pub item: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum SkipReason {
    /// The store already holds it (from an earlier import or from FMD2r itself).
    AlreadyExists,
    /// The FMD2 data could not be read or is not a valid FMD2r value.
    Invalid(String),
}

/// FMD2 data with no FMD2r counterpart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unmapped {
    /// The FMD2 file.
    pub source: String,
    /// The key or column, e.g. `general/OneInstanceOnly`.
    pub key: String,
    /// The value, as text.
    pub value: String,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dry_run {
            writeln!(f, "Dry run: nothing was written.")?;
        }
        let sources = [
            ("downloads.db → tasks", &self.tasks),
            ("favorites.db → favorites", &self.favorites),
            (
                "downloadedchapters.db → downloaded chapters",
                &self.downloaded_chapters,
            ),
            ("modules.json → module settings", &self.module_settings),
            ("modules.json → accounts", &self.accounts),
            ("settings.json → settings", &self.settings),
        ];
        for (name, source) in sources {
            if !source.found {
                writeln!(f, "{name}: not found")?;
                continue;
            }
            writeln!(
                f,
                "{name}: {} imported, {} already existing, {} invalid",
                source.imported,
                source.already_existing(),
                source.skipped.len() - source.already_existing()
            )?;
            for s in &source.skipped {
                if let SkipReason::Invalid(why) = &s.reason {
                    writeln!(f, "  skipped {}: {why}", s.item)?;
                }
            }
        }
        if !self.unmapped.is_empty() {
            writeln!(f, "Not imported (no FMD2r counterpart):")?;
            for u in &self.unmapped {
                writeln!(f, "  {} {} = {}", u.source, u.key, u.value)?;
            }
        }
        for w in &self.warnings {
            writeln!(f, "warning: {w}")?;
        }
        Ok(())
    }
}
