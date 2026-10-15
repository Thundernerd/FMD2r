//! The free-space reader behind the download engine's disk guard (issue #308).

#![allow(clippy::unwrap_used)]

use fmd_core::disk_space::{FreeSpace, Statvfs};

#[cfg(unix)]
#[test]
fn a_folder_not_created_yet_is_measured_on_the_disk_that_will_hold_it() {
    let dir = tempfile::tempdir().unwrap();
    let existing = Statvfs.free_bytes(dir.path()).unwrap();
    assert!(existing > 0);

    let later = Statvfs
        .free_bytes(&dir.path().join("Manga/Chapter"))
        .unwrap();
    // Other processes may write meanwhile; it is the same disk all the same.
    let slack = 64 * 1024 * 1024;
    assert!(later.abs_diff(existing) < slack, "{existing} vs {later}");
}

#[cfg(unix)]
#[test]
fn a_relative_folder_is_measured_on_the_working_directorys_disk() {
    let here = Statvfs.free_bytes(std::path::Path::new(".")).unwrap();
    let relative = Statvfs
        .free_bytes(std::path::Path::new("no-such-folder-308/x"))
        .unwrap();
    let slack = 64 * 1024 * 1024;
    assert!(relative.abs_diff(here) < slack, "{here} vs {relative}");
}
