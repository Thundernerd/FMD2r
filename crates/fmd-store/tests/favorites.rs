// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_store::{AppDb, NewFavorite};

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
