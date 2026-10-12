//! Building `metadata.db` from the recorded MangaBaka dump
//! (T73, issue #204).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::mangabaka::{FixtureSource, fixture_records};
use fmd_core::metadata::{MangaBakaDb, MangaDexLinks, Matcher, MetadataError, MetadataJobs};
use fmd_core::settings::SettingsService;
use fmd_http::{
    BoxFuture, HttpClient, TerminateToken, Transport, TransportError, WireRequest, WireResponse,
};
use fmd_store::{AppDb, ListsDb};

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
fn the_urls_mangabaka_lists_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();

    let series = db.current().unwrap().series(189).unwrap().unwrap();
    for url in [
        "https://www.webtoons.com/en/drama/reborn-rich/list?title_no=4956",
        "https://mangabaka.org/189",
    ] {
        assert!(series.urls.iter().any(|u| u == url), "{:?}", series.urls);
    }
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

#[test]
fn display_titles_and_credits_are_kept_as_written() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let meta = db.current().unwrap();

    let series = meta.series(189).unwrap().unwrap();
    assert_eq!(series.native_title.as_deref(), Some("재벌집 막내아들"));
    assert_eq!(
        series.romanized_title.as_deref(),
        Some("Jaebeoljib Magnaeadeul")
    );
    // Its official English entry, not the first one ("Chaebeol Jip Mangnaeadeul").
    assert_eq!(series.english_title.as_deref(), Some("Reborn Rich"));
    assert_eq!(series.authors, ["JP", "San-Gyeong"]);
    assert_eq!(series.artists, ["Byeong-Gwan Kim"]);
}

#[test]
fn the_english_title_falls_back_to_the_primary_one_and_else_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut records = fixture_records();
    // Reborn Rich without any English entry.
    let reborn = records.iter_mut().find(|r| r["id"] == 189).unwrap();
    reborn["titles"]
        .as_array_mut()
        .unwrap()
        .retain(|t| t["language"] != "en");
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&records));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let meta = db.current().unwrap();

    assert_eq!(meta.series(189).unwrap().unwrap().english_title, None);
    // Shaberi Sugita Onnanoko has no official English entry, only a primary one.
    assert_eq!(
        meta.series(74312)
            .unwrap()
            .unwrap()
            .english_title
            .as_deref(),
        Some("A Girl Who Says Too Much")
    );
    // From Today, I'm a Player has no romanized title.
    assert_eq!(meta.series(83451).unwrap().unwrap().romanized_title, None);
}

#[test]
fn a_series_cross_site_ids_can_be_looked_up() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let meta = db.current().unwrap();

    let xids = meta.xids_of(189).unwrap();
    assert!(
        xids.contains(&("anilist".to_owned(), "158766".to_owned())),
        "{xids:?}"
    );
    assert!(
        xids.contains(&("manga_updates".to_owned(), "4v8x87y".to_owned())),
        "{xids:?}"
    );
    // A null ID on a site is no ID.
    let xids = meta.xids_of(83451).unwrap();
    assert!(!xids.iter().any(|(site, _)| site == "anilist"), "{xids:?}");
}

/// Never asked: no list is matched here.
struct NoNetwork;

impl Transport for NoNetwork {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("no network in tests".into())) })
    }
}

#[tokio::test]
async fn a_v2_database_is_rebuilt_at_startup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(include_str!(
        "../../fmd-store/src/migrations/metadata_v2.sql"
    ))
    .unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    drop(conn);

    let source = FixtureSource::new(&fixture_records());
    let db = Arc::new(MangaBakaDb::open(dir.path(), source.clone()));
    // Matching acts as if there is none until the rebuild is done.
    assert!(db.current().is_none());
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    let settings = SettingsService::load(AppDb::open(dir.path().join("app.db")).unwrap()).unwrap();
    let http = HttpClient::with_transport(Arc::new(NoNetwork)).unwrap();
    let jobs = MetadataJobs::new(
        db.clone(),
        Matcher::new(
            lists.clone(),
            AppDb::open(dir.path().join("app.db")).unwrap(),
            MangaDexLinks::new(http),
        ),
        lists,
        |_: &str| None,
        Arc::new(settings),
        |_| {},
    );
    let schedule = tokio::spawn(jobs.clone().schedule());

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while db.current().is_none() || jobs.is_running() {
        assert!(std::time::Instant::now() < deadline, "no rebuild started");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    schedule.abort();

    assert_eq!(source.opened.load(Ordering::SeqCst), 1);
    assert!(
        fmd_store::MetadataDb::open(&path).is_ok(),
        "the new file is v3"
    );
    let series = db.current().unwrap().series(189).unwrap().unwrap();
    assert_eq!(series.english_title.as_deref(), Some("Reborn Rich"));
    assert!(series.urls.iter().any(|u| u == "https://mangabaka.org/189"));
}

#[test]
fn a_file_without_a_schema_version_is_not_taken_for_an_old_database() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("metadata.db"), b"").unwrap();

    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));

    assert!(db.current().is_none());
    assert!(!db.is_outdated());
}

#[test]
fn a_series_can_be_searched_for_by_title_id_or_mangabaka_url() {
    let dir = tempfile::tempdir().unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(&fixture_records()));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let meta = db.current().unwrap();
    let ids = |q: &str| -> Vec<i64> { meta.search(q).unwrap().into_iter().map(|s| s.id).collect() };

    assert!(
        ids("reborn rich").contains(&189),
        "{:?}",
        ids("reborn rich")
    );
    // A title's beginning is enough.
    assert!(ids("Reborn R").contains(&189), "{:?}", ids("Reborn R"));
    assert_eq!(ids("189"), [189]);
    assert_eq!(ids("https://mangabaka.org/189"), [189]);
    assert_eq!(ids(" mangabaka.org/189 "), [189]);
    assert!(ids("").is_empty());
    assert!(ids("no such series anywhere").is_empty());
}
