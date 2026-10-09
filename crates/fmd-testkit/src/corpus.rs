//! The upstream Lua snapshot in `fixtures/lua`, refreshed by `scripts/sync-upstream-lua.sh`.

use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Debug, thiserror::Error)]
#[error("cannot read {}: {source}", path.display())]
pub struct CorpusError {
    path: PathBuf,
    source: io::Error,
}

/// `fixtures/lua`.
pub fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/lua")
}

/// The `*.lua` files directly in `modules/`, sorted, as FMD2 scans them
/// (baseunits/lua/LuaWebsiteModules.pas:644). Upstream ships no `*.luac`.
pub fn module_files() -> Result<Vec<PathBuf>, CorpusError> {
    let dir = corpus_root().join("modules");
    let error = |source| CorpusError {
        path: dir.clone(),
        source,
    };
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).map_err(error)? {
        let path = entry.map_err(error)?.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "lua") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Runs `check` on every file, collecting all failures rather than stopping at the first.
pub fn check_each<E: Display>(
    files: &[PathBuf],
    mut check: impl FnMut(&Path) -> Result<(), E>,
) -> Result<(), CorpusReport> {
    let failures: Vec<_> = files
        .iter()
        .filter_map(|path| {
            check(path).err().map(|e| Failure {
                path: path.clone(),
                message: e.to_string(),
            })
        })
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(CorpusReport {
            checked: files.len(),
            failures,
        })
    }
}

/// `Debug` prints the `Display` report so `unwrap()` in a test shows it.
#[derive(thiserror::Error)]
pub struct CorpusReport {
    checked: usize,
    failures: Vec<Failure>,
}

struct Failure {
    path: PathBuf,
    message: String,
}

impl Display for CorpusReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} of {} modules failed:",
            self.failures.len(),
            self.checked
        )?;
        let root = corpus_root();
        for Failure { path, message } in &self.failures {
            let shown = path.strip_prefix(&root).unwrap_or(path);
            writeln!(f, "  {}: {message}", shown.display())?;
        }
        Ok(())
    }
}

impl fmt::Debug for CorpusReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}
