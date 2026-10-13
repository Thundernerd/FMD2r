//! A consistent copy of `app.db` for `GET /api/backup` (issue #270, "Seams under test").
// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fmd_store::{AppDb, NewFavorite};

fn favorite(link: &str) -> NewFavorite {
    NewFavorite {
        module_id: "mangadex".into(),
        title: format!("Series {link}"),
        link: link.into(),
        save_to: "/downloads".into(),
        cover_url: None,
    }
}

#[test]
fn the_copy_opens_as_app_db_with_the_same_rows_while_another_connection_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let db = AppDb::open(&path).unwrap();
    db.settings().set("general.language", "nl").unwrap();
    for link in ["/a", "/b", "/c"] {
        db.favorites().create(&favorite(link)).unwrap();
    }

    // Another connection to the same file keeps committing numbered rows, one per transaction.
    let writer = AppDb::open(&path).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let writing = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut n = 0u32;
            while !stop.load(Ordering::Relaxed) || n < 50 {
                writer.settings().set(&format!("w.{n}"), &n).unwrap();
                n += 1;
            }
            n
        })
    };
    while db.settings().get::<u32>("w.20").unwrap().is_none() {
        std::thread::yield_now();
    }

    let copy_dir = tempfile::tempdir().unwrap();
    let copy_path = copy_dir.path().join("app.db");
    db.back_up_to(&copy_path).unwrap();
    stop.store(true, Ordering::Relaxed);
    let written = writing.join().unwrap();

    let copy = AppDb::open(&copy_path).unwrap();
    assert_eq!(copy.schema_version().unwrap(), db.schema_version().unwrap());
    assert_eq!(
        copy.settings().get::<String>("general.language").unwrap(),
        Some("nl".into())
    );
    let links: Vec<String> = copy
        .favorites()
        .list()
        .unwrap()
        .into_iter()
        .map(|f| f.link)
        .collect();
    assert_eq!(links, ["/a", "/b", "/c"]);
    // A snapshot: every row up to some commit, and none after it.
    let present: Vec<bool> = (0..written)
        .map(|n| {
            copy.settings()
                .get::<u32>(&format!("w.{n}"))
                .unwrap()
                .is_some()
        })
        .collect();
    let kept = present.iter().take_while(|p| **p).count();
    assert!(kept > 20, "only {kept} of the committed rows were copied");
    assert!(
        present[kept..].iter().all(|p| !p),
        "the copy mixes rows from different moments"
    );
}

#[test]
fn an_existing_file_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let target = dir.path().join("taken.db");
    std::fs::write(&target, b"keep me").unwrap();

    assert!(db.back_up_to(&target).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
}
