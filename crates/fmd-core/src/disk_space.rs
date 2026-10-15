//! Free space on the disks downloads save to, so the engine can hold downloads before a full
//! disk fails them (issue #308). No FMD2 counterpart: FMD2 never checks free space.

use std::path::Path;

/// Reads how much space is free on the disk holding a folder.
pub trait FreeSpace: Send + Sync {
    /// Bytes free to an unprivileged user on the disk holding `dir`, which need not exist yet;
    /// `None` when it can't be measured.
    fn free_bytes(&self, dir: &Path) -> Option<u64>;
}

/// The operating system's view, through `statvfs` (Linux and macOS). Measures the nearest
/// folder of `dir` that exists; a relative `dir` is under the working directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct Statvfs;

impl FreeSpace for Statvfs {
    #[cfg(unix)]
    fn free_bytes(&self, dir: &Path) -> Option<u64> {
        let existing = dir
            .ancestors()
            .map(|p| {
                if p.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    p
                }
            })
            .find(|p| p.is_dir())?;
        let stat = rustix::fs::statvfs(existing).ok()?;
        Some(stat.f_bavail.saturating_mul(stat.f_frsize))
    }

    #[cfg(not(unix))]
    fn free_bytes(&self, _dir: &Path) -> Option<u64> {
        None
    }
}
