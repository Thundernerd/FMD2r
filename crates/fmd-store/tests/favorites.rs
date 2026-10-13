// unwrap is fine in tests/, but clippy.toml's exemption misses helpers outside #[test] fns.
#![allow(clippy::unwrap_used)]

use fmd_store::{AppDb, FavoriteInfo, NewFavorite};

fn new_favorite(link: &str) -> NewFavorite {
    NewFavorite {
        module_id: "mangadex".into(),
        link: link.into(),
        title: format!("Title {link}"),
        save_to: "/library".into(),
        cover_url: Some("https://img/cover.jpg".into()),
    }
}

#[test]
fn favorites_create_update_reorder_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let favorites = db.favorites();

    let a = favorites.create(&new_favorite("/a")).unwrap();
    let b = favorites.create(&new_favorite("/b")).unwrap();
    assert!(a.enabled);
    assert_eq!(a.current_chapter, 0);
    assert!(
        favorites.create(&new_favorite("/a")).is_err(),
        "module+link is unique"
    );

    let mut checked = b.clone();
    checked.current_chapter = 42;
    checked.status = "Ongoing".into();
    checked.date_last_checked = Some(1_700_000_000_000);
    favorites.update(&checked).unwrap();
    assert_eq!(favorites.get(b.id).unwrap(), Some(checked.clone()));
    assert_eq!(
        favorites.find("MangaDex", "/b").unwrap(),
        None,
        "lookup is exact"
    );
    assert_eq!(
        favorites.find("mangadex", "/b").unwrap(),
        Some(checked.clone())
    );

    favorites.reorder(&[b.id, a.id]).unwrap();
    let order: Vec<_> = favorites
        .list()
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(order, [b.id, a.id]);

    favorites.delete(a.id).unwrap();
    let remaining = favorites.list().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, b.id);
    assert_eq!(remaining[0].current_chapter, 42);
}

#[test]
fn saving_a_favorites_website_info_survives_a_rename() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let favorites = db.favorites();
    let mut favorite = favorites.create(&new_favorite("/a")).unwrap();
    let info = FavoriteInfo {
        title: "Reborn Rich".into(),
        alt_titles: "재벌집 막내아들".into(),
        authors: "SAN.G".into(),
        artists: "JP".into(),
        genres: "Drama, Fantasy".into(),
    };

    assert!(favorites.set_info(favorite.id, &info).unwrap(), "changed");
    assert!(
        !favorites.set_info(favorite.id, &info).unwrap(),
        "unchanged"
    );
    favorite.title = "My Name For It".into();
    favorites.update(&favorite).unwrap();

    assert_eq!(favorites.info(favorite.id).unwrap(), Some(info));
    assert_eq!(
        favorites.get(favorite.id).unwrap().unwrap().title,
        "My Name For It"
    );
}

#[test]
fn changing_only_a_favorites_website_genres_is_saved_but_not_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let favorites = db.favorites();
    let favorite = favorites.create(&new_favorite("/a")).unwrap();
    let mut info = FavoriteInfo {
        title: "Reborn Rich".into(),
        alt_titles: String::new(),
        authors: "SAN.G".into(),
        artists: "JP".into(),
        genres: "Drama".into(),
    };
    assert!(favorites.set_info(favorite.id, &info).unwrap(), "changed");

    info.genres = "Drama, Fantasy".into();

    assert!(
        !favorites.set_info(favorite.id, &info).unwrap(),
        "a genres-only change is not a change"
    );
    assert_eq!(favorites.info(favorite.id).unwrap(), Some(info));
}

#[test]
fn groups_create_rename_reorder_and_reject_a_name_in_another_case() {
    let db = AppDb::open(":memory:").unwrap();
    let groups = db.library_groups();

    let shounen = groups.create("Shounen").unwrap();
    let seinen = groups.create("Seinen").unwrap();
    assert_eq!(shounen.name, "Shounen");
    assert!(!shounen.collapsed);
    assert!(groups.create("shounen").is_err(), "names ignore case");

    let mut renamed = seinen.clone();
    renamed.name = "Seinen (done)".into();
    renamed.collapsed = true;
    groups.update(&renamed).unwrap();
    assert_eq!(groups.get(seinen.id).unwrap(), Some(renamed.clone()));
    let mut clash = renamed.clone();
    clash.name = "SHOUNEN".into();
    assert!(groups.update(&clash).is_err(), "rename into a taken name");

    groups.reorder(&[seinen.id, shounen.id]).unwrap();
    let names: Vec<_> = groups.list().unwrap().into_iter().map(|g| g.name).collect();
    assert_eq!(names, ["Seinen (done)", "Shounen"]);
}

#[test]
fn deleting_a_group_ungroups_its_favorites() {
    let db = AppDb::open(":memory:").unwrap();
    let group = db.library_groups().create("Shounen").unwrap();
    let mut favorite = db.favorites().create(&new_favorite("/a")).unwrap();
    assert_eq!(favorite.group_id, None, "new favorites are ungrouped");

    favorite.group_id = Some(group.id);
    db.favorites().update(&favorite).unwrap();
    assert_eq!(
        db.favorites().get(favorite.id).unwrap(),
        Some(favorite.clone())
    );

    db.library_groups().delete(group.id).unwrap();
    assert!(db.library_groups().list().unwrap().is_empty());
    let kept = db.favorites().get(favorite.id).unwrap().unwrap();
    assert_eq!(kept.group_id, None);
}

#[test]
fn failed_checks_count_up_keeping_the_last_error_until_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let favorites = db.favorites();
    let a = favorites.create(&new_favorite("/a")).unwrap();
    let b = favorites.create(&new_favorite("/b")).unwrap();
    assert_eq!(a.check_failures, 0);
    assert_eq!(a.last_check_error, None);
    assert_eq!(a.date_last_failed, None);

    favorites
        .record_check_failure(a.id, "404 Not Found", 1_700_000_000_000)
        .unwrap();
    favorites
        .record_check_failure(a.id, "connection refused", 1_700_000_060_000)
        .unwrap();

    let failing = favorites.get(a.id).unwrap().unwrap();
    assert_eq!(failing.check_failures, 2);
    assert_eq!(
        failing.last_check_error.as_deref(),
        Some("connection refused")
    );
    assert_eq!(failing.date_last_failed, Some(1_700_000_060_000));
    assert_eq!(
        favorites.get(b.id).unwrap().unwrap().check_failures,
        0,
        "only that favorite"
    );

    // An edit leaves the failures as they are.
    let mut renamed = failing.clone();
    renamed.title = "Renamed".into();
    favorites.update(&renamed).unwrap();
    assert_eq!(favorites.get(a.id).unwrap().unwrap().check_failures, 2);

    favorites.clear_check_failures(a.id).unwrap();
    let cleared = favorites.get(a.id).unwrap().unwrap();
    assert_eq!(cleared.check_failures, 0);
    assert_eq!(cleared.last_check_error, None);
    assert_eq!(cleared.date_last_failed, None);
}
