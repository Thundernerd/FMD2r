// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

//! Moving a library series' downloaded chapters to its new folder (#283).

use std::fs;
use std::path::Path;

use fmd_core::folder_move::{FolderMover, LibraryFolders, SharedFolder};

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn moves_a_series_folders_files_and_removes_the_empty_source() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old/Series");
    let new = root.path().join("new/Series");
    write(&old.join("Chapter 1/001.jpg"), "page 1");
    write(&old.join("Chapter 2.cbz"), "chapter 2");

    let report = FolderMover::default().move_folder(&old, &new);

    assert_eq!(read(&new.join("Chapter 1/001.jpg")), "page 1");
    assert_eq!(read(&new.join("Chapter 2.cbz")), "chapter 2");
    assert!(!old.exists(), "the emptied old folder is removed");
    assert_eq!(report.moved, 2);
    assert!(report.skipped.is_empty());
    assert!(report.failures.is_empty());
}

#[test]
fn skips_and_reports_a_file_already_in_the_new_folder_without_touching_it() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    write(&old.join("Chapter 1.cbz"), "old copy");
    write(&old.join("Chapter 2.cbz"), "chapter 2");
    write(&new.join("Chapter 1.cbz"), "new copy");

    let report = FolderMover::default().move_folder(&old, &new);

    assert_eq!(read(&new.join("Chapter 1.cbz")), "new copy");
    assert_eq!(read(&old.join("Chapter 1.cbz")), "old copy");
    assert_eq!(read(&new.join("Chapter 2.cbz")), "chapter 2");
    assert_eq!(report.moved, 1);
    assert_eq!(report.skipped, vec![old.join("Chapter 1.cbz")]);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
}

#[test]
fn copies_then_deletes_when_a_rename_fails_as_across_file_systems() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    write(&old.join("Chapter 1/001.jpg"), "page 1");
    write(&old.join("Chapter 1/002.jpg"), "page 2");
    write(&old.join("Chapter 2.cbz"), "chapter 2");
    let cross_device = FolderMover::with_rename(|_, _| {
        Err(std::io::Error::new(
            std::io::ErrorKind::CrossesDevices,
            "Invalid cross-device link",
        ))
    });

    let report = cross_device.move_folder(&old, &new);

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(read(&new.join("Chapter 1/001.jpg")), "page 1");
    assert_eq!(read(&new.join("Chapter 1/002.jpg")), "page 2");
    assert_eq!(read(&new.join("Chapter 2.cbz")), "chapter 2");
    assert!(
        !old.exists(),
        "the copied files are deleted, then the folder"
    );
    assert_eq!(report.moved, 2);
}

#[test]
fn a_missing_old_folder_is_nothing_to_move() {
    let root = tempfile::tempdir().unwrap();
    let new = root.path().join("new");

    let report = FolderMover::default().move_folder(&root.path().join("never downloaded"), &new);

    assert_eq!(report, Default::default());
    assert!(!new.exists(), "no empty new folder is made");
}

#[test]
fn the_same_folder_is_nothing_to_move() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("Series");
    write(&folder.join("Chapter 1.cbz"), "chapter 1");

    let report = FolderMover::default().move_folder(&folder, &root.path().join("./Series/"));

    assert_eq!(report, Default::default());
    assert_eq!(read(&folder.join("Chapter 1.cbz")), "chapter 1");
}

#[test]
fn refuses_a_folder_shared_with_another_series_or_a_destination() {
    let root = tempfile::tempdir().unwrap();
    let downloads = root.path().join("downloads");
    let new = root.path().join("new");
    write(&downloads.join("Other/Chapter 1.cbz"), "other series");
    write(&downloads.join("Chapter 1.cbz"), "this series");
    write(
        &root.path().join("parent/Other/Chapter 1.cbz"),
        "a third series",
    );
    let library = LibraryFolders {
        series: vec![
            downloads.join("Other"),
            root.path().join("same"),
            root.path().join("parent/Other"),
        ],
        destinations: vec![downloads.clone()],
    };
    let mover = FolderMover::default();

    // "Generate manga folder" off: the series downloaded straight into the destination.
    assert_eq!(
        mover.move_series(&downloads, &new, &library),
        Err(SharedFolder::Destination(downloads.clone()))
    );
    assert_eq!(
        mover.move_series(&root.path().join("same"), &new, &library),
        Err(SharedFolder::Series(root.path().join("same")))
    );
    assert_eq!(
        mover.move_series(&root.path().join("parent"), &new, &library),
        Err(SharedFolder::Series(root.path().join("parent/Other")))
    );
    assert_eq!(read(&downloads.join("Other/Chapter 1.cbz")), "other series");
    assert_eq!(read(&downloads.join("Chapter 1.cbz")), "this series");
    assert_eq!(
        read(&root.path().join("parent/Other/Chapter 1.cbz")),
        "a third series"
    );
    assert!(!new.exists());
}

#[test]
fn moves_a_series_whose_folder_no_other_series_shares() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("downloads/Series");
    let new = root.path().join("new/Series");
    write(&old.join("Chapter 1.cbz"), "chapter 1");
    let library = LibraryFolders {
        series: vec![root.path().join("downloads/Other")],
        destinations: vec![root.path().join("downloads")],
    };

    let report = FolderMover::default()
        .move_series(&old, &new, &library)
        .unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(read(&new.join("Chapter 1.cbz")), "chapter 1");
}

#[test]
fn a_new_folder_inside_the_old_one_takes_the_rest_of_it() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("Series");
    let new = old.join("Moved");
    write(&old.join("Chapter 1.cbz"), "chapter 1");
    let cross_device = FolderMover::with_rename(|_, _| Err(std::io::Error::other("no rename")));

    let report = cross_device.move_folder(&old, &new);

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.moved, 1);
    assert_eq!(read(&new.join("Chapter 1.cbz")), "chapter 1");
    assert!(!new.join("Moved").exists());
}
