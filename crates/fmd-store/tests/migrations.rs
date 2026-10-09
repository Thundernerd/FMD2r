// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_store::{AppDb, ListsDb};

fn tables(path: &std::path::Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type IN ('table') ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn fresh_app_db_runs_all_migrations_and_reopen_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");

    let db = AppDb::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 4);
    drop(db);

    let db = AppDb::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 4);
    drop(db);

    let names = tables(&path);
    for t in [
        "tasks",
        "task_chapters",
        "task_pages",
        "favorites",
        "downloaded_chapters",
        "module_settings",
        "accounts",
        "settings",
        "events",
        "module_files",
        "sessions",
        "favorite_chapters",
    ] {
        assert!(names.iter().any(|n| n == t), "missing table {t}: {names:?}");
    }
}

#[test]
fn fresh_lists_db_runs_all_migrations_and_reopen_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lists.db");

    let db = ListsDb::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 2);
    drop(db);
    let db = ListsDb::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 2);
    drop(db);

    let names = tables(&path);
    assert!(names.iter().any(|n| n == "masterlist"));
    assert!(names.iter().any(|n| n == "masterlist_fts"));
}

#[test]
fn opening_a_db_newer_than_this_build_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    drop(AppDb::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    assert!(matches!(
        AppDb::open(&path),
        Err(fmd_store::StoreError::SchemaTooNew { found: 99, .. })
    ));
}
