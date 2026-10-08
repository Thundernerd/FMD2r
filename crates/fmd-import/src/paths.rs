//! Save-to path translation (FMD2's Windows paths to the FMD2r host's).

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::report::ImportReport;

/// Rewrites paths under `from` to the same place under `to`, e.g. `C:\Manga` → `/data/manga`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Error)]
#[error("expected FROM=TO, got {0:?}")]
pub struct PathMapParseError(String);

impl FromStr for PathMap {
    type Err = PathMapParseError;

    /// Parses `FROM=TO` (split at the first `=`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('=') {
            Some((from, to)) if !from.is_empty() => Ok(Self {
                from: from.to_string(),
                to: to.to_string(),
            }),
            _ => Err(PathMapParseError(s.to_string())),
        }
    }
}

impl fmt::Display for PathMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.from, self.to)
    }
}

/// A Windows path: `X:\…`, `X:/…` or a UNC `\\server\…` path.
fn is_windows(path: &str) -> bool {
    let b = path.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
        || path.starts_with("\\\\")
}

/// `path` with `/` separators, without trailing ones.
fn normalize(path: &str) -> String {
    let path = path.replace('\\', "/");
    match path.trim_end_matches('/') {
        "" => path,
        trimmed => trimmed.to_string(),
    }
}

impl PathMap {
    /// The part of `path` after `from`, with `/` separators, when `path` is `from` or lies under
    /// it. Windows paths compare case-insensitively.
    fn rest_of(&self, path: &str) -> Option<String> {
        let from = normalize(&self.from);
        let path = path.replace('\\', "/");
        let head = path.get(..from.len())?;
        let same = if is_windows(&self.from) {
            head.eq_ignore_ascii_case(&from)
        } else {
            head == from
        };
        let rest = &path[from.len()..];
        (same && (rest.is_empty() || rest.starts_with('/') || from.ends_with('/')))
            .then(|| rest.to_string())
    }
}

/// Applies the map with the longest matching `from` to `path`, or returns `path` unchanged; an
/// untranslated Windows path adds a warning to the report.
pub(crate) fn translate(maps: &[PathMap], path: &str, report: &mut ImportReport) -> String {
    let best = maps
        .iter()
        .filter_map(|m| Some((normalize(&m.from).len(), m, m.rest_of(path)?)))
        .max_by_key(|(len, _, _)| *len);
    match best {
        Some((_, map, rest)) => {
            let to = map.to.trim_end_matches('/');
            if to.is_empty() && rest.is_empty() {
                map.to.clone()
            } else {
                format!("{to}{rest}")
            }
        }
        None => {
            if is_windows(path) {
                let warning =
                    format!("save-to path {path} is a Windows path that no path map covers");
                if !report.warnings.contains(&warning) {
                    report.warnings.push(warning);
                }
            }
            path.to_string()
        }
    }
}
