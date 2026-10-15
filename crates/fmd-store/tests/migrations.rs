// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
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
    assert_eq!(db.schema_version().unwrap(), 11);
    drop(db);

    let db = AppDb::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 11);
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
        "metadata_overrides",
        "library_groups",
    ] {
        assert!(names.iter().any(|n| n == t), "missing table {t}: {names:?}");
    }
}

#[test]
fn fresh_lists_db_runs_all_migrations_and_reopen_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lists.db");

    let db = ListsDb::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 5);
    drop(db);
    let db = ListsDb::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 5);
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

#[test]
fn upgrading_lists_db_forgets_website_no_cover_links_only() {
    use fmd_store::{CoverLink, CoverSource};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lists.db");
    let link = |url: Option<&str>, source| CoverLink {
        url: url.map(str::to_owned),
        large_url: None,
        source,
        series_id: None,
        checked_at: 1_760_210_700,
    };
    let rows = [
        ("/blocked", link(None, CoverSource::Website)),
        (
            "/website",
            link(Some("https://site.test/c.jpg"), CoverSource::Website),
        ),
        (
            "/mangabaka",
            link(Some("https://cdn.test/c.jpg"), CoverSource::MangaBaka),
        ),
        ("/mangabaka-none", link(None, CoverSource::MangaBaka)),
    ];
    let db = ListsDb::open(&path).unwrap();
    for (l, cover) in &rows {
        db.cover_links().put("site", l, cover).unwrap();
    }
    drop(db);
    // As stored by a v4 build, before the website "no cover" rows were cleared (issue #233).
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 4)
        .unwrap();

    let db = ListsDb::open(&path).unwrap();

    assert_eq!(db.cover_links().get("site", "/blocked").unwrap(), None);
    for (l, cover) in &rows[1..] {
        assert_eq!(
            db.cover_links().get("site", l).unwrap().as_ref(),
            Some(cover),
            "{l}"
        );
    }
}

#[test]
fn upgrading_app_db_gives_favorites_empty_website_info() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    // As stored by a v4 build.
    let conn = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../src/migrations/app_v1.sql"),
        include_str!("../src/migrations/app_v2.sql"),
        include_str!("../src/migrations/app_v3.sql"),
        include_str!("../src/migrations/app_v4.sql"),
    ] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute(
        "INSERT INTO favorites (module_id, link, title, save_to, sort_order, date_added)
         VALUES ('site', '/a', 'A Title', '/library', 1, 1760210700000)",
        [],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 4).unwrap();
    drop(conn);

    let db = AppDb::open(&path).unwrap();

    let favorite = db.favorites().find("site", "/a").unwrap().unwrap();
    assert_eq!(favorite.title, "A Title");
    assert_eq!(
        db.favorites().info(favorite.id).unwrap(),
        Some(fmd_store::FavoriteInfo::default())
    );
}

#[test]
fn upgrading_app_db_gives_favorites_empty_website_genres() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    // As stored by a v7 build.
    let conn = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../src/migrations/app_v1.sql"),
        include_str!("../src/migrations/app_v2.sql"),
        include_str!("../src/migrations/app_v3.sql"),
        include_str!("../src/migrations/app_v4.sql"),
        include_str!("../src/migrations/app_v5.sql"),
        include_str!("../src/migrations/app_v6.sql"),
        include_str!("../src/migrations/app_v7.sql"),
    ] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute(
        "INSERT INTO favorites (module_id, link, title, save_to, sort_order, date_added,
            info_title, authors)
         VALUES ('site', '/a', 'A Title', '/library', 1, 1760210700000, 'A Title', 'Someone')",
        [],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 7).unwrap();
    drop(conn);

    let db = AppDb::open(&path).unwrap();

    let favorite = db.favorites().find("site", "/a").unwrap().unwrap();
    let info = db.favorites().info(favorite.id).unwrap().unwrap();
    assert_eq!(info.authors, "Someone");
    assert_eq!(info.genres, "");
}

#[test]
fn upgrading_app_db_keeps_groups_filled_by_hand_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    // As stored by a v8 build.
    let conn = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../src/migrations/app_v1.sql"),
        include_str!("../src/migrations/app_v2.sql"),
        include_str!("../src/migrations/app_v3.sql"),
        include_str!("../src/migrations/app_v4.sql"),
        include_str!("../src/migrations/app_v5.sql"),
        include_str!("../src/migrations/app_v6.sql"),
        include_str!("../src/migrations/app_v7.sql"),
        include_str!("../src/migrations/app_v8.sql"),
    ] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute(
        "INSERT INTO library_groups (name, sort_order) VALUES ('Shounen', 1)",
        [],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 8).unwrap();
    drop(conn);

    let db = AppDb::open(&path).unwrap();

    let groups = db.library_groups().list().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "Shounen");
    assert_eq!(groups[0].rule, None);
}

#[test]
fn a_match_picked_by_hand_is_stored_as_manual_and_accepted() {
    use fmd_store::MatchConfidence;
    assert_eq!(MatchConfidence::Manual.as_str(), "manual");
    assert_eq!(
        MatchConfidence::parse("manual"),
        Some(MatchConfidence::Manual)
    );
    assert!(MatchConfidence::Manual.is_accepted());
}

#[test]
fn upgrading_app_db_gives_favorites_no_check_failures() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    // As stored by a v9 build.
    let conn = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../src/migrations/app_v1.sql"),
        include_str!("../src/migrations/app_v2.sql"),
        include_str!("../src/migrations/app_v3.sql"),
        include_str!("../src/migrations/app_v4.sql"),
        include_str!("../src/migrations/app_v5.sql"),
        include_str!("../src/migrations/app_v6.sql"),
        include_str!("../src/migrations/app_v7.sql"),
        include_str!("../src/migrations/app_v8.sql"),
        include_str!("../src/migrations/app_v9.sql"),
    ] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute(
        "INSERT INTO favorites (module_id, link, title, save_to, sort_order, date_added)
         VALUES ('site', '/a', 'A Title', '/library', 1, 1760210700000)",
        [],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 9).unwrap();
    drop(conn);

    let db = AppDb::open(&path).unwrap();

    let favorite = db.favorites().find("site", "/a").unwrap().unwrap();
    assert_eq!(favorite.check_failures, 0);
    assert_eq!(favorite.last_check_error, None);
    assert_eq!(favorite.date_last_failed, None);
}
