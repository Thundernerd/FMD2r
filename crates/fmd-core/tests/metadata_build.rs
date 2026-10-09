//! Building `metadata.db` from the recorded MangaBaka dump
//! (docs/tickets/T73-mangabaka-metadata.md).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use common::mangabaka::{FixtureSource, fixture_records};
use fmd_core::metadata::{MangaBakaDb, MetadataError};
use fmd_http::TerminateToken;

#[test]
fn active_series_are_kept_with_their_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    assert!(db.current().is_none(), "nothing until the first download");

    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();

    let meta = db.current().unwrap();
    let series = meta.series(2092).unwrap().unwrap();
    assert_eq!(series.title, "Shadow Star☆");
    assert_eq!(series.kind, "manga");
    assert_eq!(series.status, "completed");
    assert_eq!(series.year, Some(1998));
    assert_eq!(series.content_rating, "safe");
    assert!(series.description.starts_with("Shiina Tamai"));
    assert!(series.genres.contains(&"Psychological".to_owned()));
    assert!(
        series
            .cover_x250
            .as_deref()
            .unwrap()
            .starts_with("https://cdn.mangabaka.dev/imgproxy/plain/x250@1/")
    );
    // Any of its titles, in any language, finds it.
    assert_eq!(meta.find_by_title("Narutaru").unwrap(), vec![2092]);
    assert_eq!(meta.find_by_title("なるたる").unwrap(), vec![2092]);
    assert_eq!(meta.find_by_xid("anilist", "31153").unwrap(), vec![2092]);
    assert_eq!(
        meta.find_by_link("https://www.webtoons.com/en/drama/reborn-rich/list?title_no=4956")
            .unwrap(),
        vec![189]
    );

    let info = db.info().unwrap();
    assert!(info.bytes > 0);
    assert!(info.built_at > 0);
}

#[test]
fn a_merged_series_points_at_the_series_it_was_merged_into() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let meta = db.current().unwrap();

    // 728, "One Piece (Official Colored)", was merged into 377, "ONE PIECE".
    assert!(meta.series(728).unwrap().is_none());
    assert_eq!(
        meta.find_by_title("One Piece (Official Colored)").unwrap(),
        vec![377]
    );
    // Both carry AniList 30013: one series.
    assert_eq!(meta.find_by_xid("anilist", "30013").unwrap(), vec![377]);
}

#[test]
fn a_deleted_series_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let meta = db.current().unwrap();

    assert!(meta.series(952).unwrap().is_none());
    assert!(meta.find_by_title("Failed Princesses").unwrap().is_empty());
}

#[test]
fn a_missing_field_fails_the_build_and_keeps_the_old_database() {
    let dir = tempfile::tempdir().unwrap();
    let good = fixture_records();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&good));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let before = db.info().unwrap();

    let mut broken = good.clone();
    broken[3].as_object_mut().unwrap().remove("title");
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&broken));
    let err = db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap_err();

    assert!(
        matches!(&err, MetadataError::MissingField { field, id: Some(2092), .. } if field == "title"),
        "{err:?}"
    );
    assert!(err.to_string().contains("title"), "{err}");
    assert_eq!(db.info().unwrap(), before);
    let meta = db.current().unwrap();
    assert_eq!(meta.find_by_title("Shadow Star").unwrap(), vec![2092]);
    // No half-built file is left next to it.
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files, vec!["metadata.db".to_owned()]);
}

#[test]
fn fields_mangabaka_adds_are_tolerated() {
    let dir = tempfile::tempdir().unwrap();
    let mut records = fixture_records();
    records[0].as_object_mut().unwrap().insert(
        "something_new".into(),
        serde_json::json!({ "nested": [1, 2] }),
    );
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&records));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    assert_eq!(
        db.current().unwrap().series(189).unwrap().unwrap().title,
        "Reborn Rich"
    );
}

#[test]
fn removing_deletes_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();

    db.remove().unwrap();

    assert!(db.current().is_none());
    assert!(db.info().is_none());
    assert!(!dir.path().join("metadata.db").exists());
}

#[test]
fn a_cancelled_build_stops_and_keeps_the_old_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let before = db.info().unwrap();

    // Cancelled while the download is being read.
    let terminate = TerminateToken::new();
    terminate.terminate();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = db.refresh(&terminate, &mut |_| {});
        let _ = tx.send((db, result));
    });
    let (db, result) = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the cancelled build stops");

    assert!(
        matches!(result, Err(MetadataError::Cancelled)),
        "{result:?}"
    );
    assert_eq!(db.info().unwrap(), before);
}
