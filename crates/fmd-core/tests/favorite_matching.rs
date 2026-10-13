//! Favorites matched against `metadata.db` without their website's list (issue #242, "Seams
//! under test"), over the recorded dump (tests/fixtures/mangabaka).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::mangabaka::{FixtureSource, fixture_records};
use fmd_core::favorites::{TagsAndFormat, tags_and_format};
use fmd_core::metadata::{MangaBakaDb, MangaDexLinks, Matcher, MetadataJobs};
use fmd_core::settings::SettingsService;
use fmd_http::{
    BoxFuture, HttpClient, TerminateToken, Transport, TransportError, WireRequest, WireResponse,
};
use fmd_store::{
    AppDb, Favorite, FavoriteInfo, ListsDb, MangaListing, MatchConfidence, NewFavorite,
};
use serde_json::json;

const FIRE: &str = "23eb3a472201427e8824ecdd5223bad7";
const MANGADEX: &str = "d07c9c2425764da8ba056505f57cf40c";
const SCHOLAR: &str = "/title/f2cb9465-c314-46a3-97fd-05210d3af0a7";

/// MangaDex's `GET /manga?ids[]=...`, counting the requests: links The Scholar's Reincarnation
/// to MangaUpdates and MyAnimeList (results-72.csv).
#[derive(Default)]
struct MangaDexApi {
    requests: AtomicUsize,
}

impl Transport for MangaDexApi {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let body = json!({ "result": "ok", "response": "collection", "data": [{
            "id": "f2cb9465-c314-46a3-97fd-05210d3af0a7",
            "type": "manga",
            "attributes": { "links": { "mu": "mxtfjrx", "mal": "147924" } },
        }]});
        let response = WireResponse {
            status: 200,
            reason: "OK".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.to_string().into_bytes(),
        };
        Box::pin(async move { Ok(response) })
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    app: AppDb,
    lists: ListsDb,
    jobs: MetadataJobs,
    mangadex: Arc<MangaDexApi>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let app = AppDb::open(dir.path().join("app.db")).unwrap();
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    // Updated once, but empty.
    lists
        .masterlist()
        .replace_module(FIRE, std::iter::empty::<MangaListing>())
        .unwrap();
    let db = Arc::new(MangaBakaDb::open(
        dir.path(),
        FixtureSource::new(&fixture_records()),
    ));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    let mangadex = Arc::new(MangaDexApi::default());
    let http = HttpClient::with_transport(mangadex.clone()).unwrap();
    let settings = SettingsService::load(app.clone()).unwrap();
    let jobs = MetadataJobs::new(
        db,
        Matcher::new(lists.clone(), app.clone(), MangaDexLinks::new(http)),
        lists.clone(),
        |id: &str| match id {
            FIRE => Some("https://mangafire.to".to_owned()),
            MANGADEX => Some("https://mangadex.org".to_owned()),
            _ => None,
        },
        Arc::new(settings),
        |_| {},
    );
    Fixture {
        _dir: dir,
        app,
        lists,
        jobs,
        mangadex,
    }
}

impl Fixture {
    fn favorite(&self, module_id: &str, link: &str, info: &FavoriteInfo) -> Favorite {
        let favorite = self
            .app
            .favorites()
            .create(&NewFavorite {
                module_id: module_id.into(),
                link: link.into(),
                title: info.title.clone(),
                save_to: "/library".into(),
                cover_url: None,
            })
            .unwrap();
        self.app.favorites().set_info(favorite.id, info).unwrap();
        favorite
    }

    fn series_id(&self, module_id: &str, link: &str) -> Option<i64> {
        let m = self.lists.matches().get(module_id, link).unwrap()?;
        assert!(m.confidence.is_accepted(), "{m:?}");
        m.series_id
    }
}

fn reborn_rich() -> FavoriteInfo {
    FavoriteInfo {
        title: "Reborn Rich".into(),
        authors: "San-Gyeong, JP".into(),
        ..FavoriteInfo::default()
    }
}

fn scholar() -> FavoriteInfo {
    FavoriteInfo {
        title: "The Scholar's Reincarnation".into(),
        authors: "Yu Hyun So (소유현)".into(),
        ..FavoriteInfo::default()
    }
}

#[test]
fn a_favorite_matches_without_its_websites_list() {
    let f = fixture();
    f.favorite(FIRE, "/manga/reborn-rich", &reborn_rich());

    f.jobs.catch_up();

    let m = f
        .lists
        .matches()
        .get(FIRE, "/manga/reborn-rich")
        .unwrap()
        .unwrap();
    assert_eq!(m.confidence, MatchConfidence::TitleAuthor);
    assert_eq!(m.series_id, Some(189));
    assert_eq!(m.format.as_deref(), Some("manhwa"));
    assert_eq!(m.year, Some(2022));
}

#[test]
fn a_matched_favorites_tags_add_mangabaka_genres_without_case_duplicates() {
    let f = fixture();
    let favorite = f.favorite(FIRE, "/manga/reborn-rich", &reborn_rich());
    f.jobs.catch_up();

    let tagged = tags_and_format(Some(&f.jobs), &favorite, " drama, Action ,, Revenge");

    // MangaBaka's Reborn Rich (189): Drama, Fantasy, Psychological, Seinen; manhwa.
    assert_eq!(
        tagged,
        TagsAndFormat {
            tags: vec![
                "drama".into(),
                "Action".into(),
                "Revenge".into(),
                "Fantasy".into(),
                "Psychological".into(),
                "Seinen".into(),
            ],
            format: Some("manhwa".into()),
        }
    );
}

#[test]
fn a_series_matched_by_hand_takes_the_format_of_the_picked_series() {
    let f = fixture();
    let favorite = f.favorite(
        FIRE,
        "/manga/skip-loafer",
        &FavoriteInfo {
            title: "Some Other Name".into(),
            ..FavoriteInfo::default()
        },
    );
    f.jobs.catch_up();
    // MangaBaka's Skip and Loafer (86) is a manga.
    f.jobs.set_override(FIRE, "/manga/skip-loafer", 86).unwrap();

    let tagged = tags_and_format(Some(&f.jobs), &favorite, "");

    assert_eq!(tagged.format.as_deref(), Some("manga"));
    assert!(
        tagged.tags.contains(&"Slice of Life".to_owned()),
        "{tagged:?}"
    );
}

#[test]
fn without_a_match_or_a_database_a_favorite_has_only_its_websites_genres() {
    let f = fixture();
    let favorite = f.favorite(
        FIRE,
        "/manga/unknown",
        &FavoriteInfo {
            title: "Nothing MangaBaka Knows".into(),
            ..FavoriteInfo::default()
        },
    );
    f.jobs.catch_up();
    let expected = TagsAndFormat {
        tags: vec!["Action".into(), "Comedy".into()],
        format: None,
    };

    assert_eq!(
        tags_and_format(Some(&f.jobs), &favorite, "Action, Comedy, action"),
        expected,
        "no match"
    );
    assert_eq!(
        tags_and_format(None, &favorite, "Action, Comedy, action"),
        expected,
        "no database"
    );
}

#[test]
fn a_favorite_is_matched_again_only_when_what_it_matches_on_changed() {
    let f = fixture();
    let mut info = scholar();
    let favorite = f.favorite(MANGADEX, SCHOLAR, &info);
    f.jobs.catch_up();
    // MangaDex's cross-site IDs apply to favorites too (results-72.csv: 2723).
    assert_eq!(f.series_id(MANGADEX, SCHOLAR), Some(2723));
    assert_eq!(f.mangadex.requests.load(Ordering::SeqCst), 1);

    f.jobs.catch_up();
    assert_eq!(
        f.mangadex.requests.load(Ordering::SeqCst),
        1,
        "nothing changed, so not matched again"
    );

    info.alt_titles = "Reborn as a Scholar".into();
    f.app.favorites().set_info(favorite.id, &info).unwrap();
    f.jobs.catch_up();
    assert_eq!(f.mangadex.requests.load(Ordering::SeqCst), 2);
    assert_eq!(f.series_id(MANGADEX, SCHOLAR), Some(2723));
}

#[test]
fn a_favorites_match_survives_list_updates_until_it_is_removed() {
    let f = fixture();
    let favorite = f.favorite(MANGADEX, SCHOLAR, &scholar());
    f.jobs.catch_up();
    assert_eq!(f.mangadex.requests.load(Ordering::SeqCst), 1);
    // A list update prunes the module's matches, then matches what is pending.
    f.lists
        .masterlist()
        .replace_module(MANGADEX, [listing("/other", "One Piece")])
        .unwrap();
    f.jobs.list_changed(MANGADEX, &TerminateToken::new());
    assert_eq!(f.series_id(MANGADEX, SCHOLAR), Some(2723));
    assert_eq!(
        f.mangadex.requests.load(Ordering::SeqCst),
        1,
        "kept, so not matched again"
    );

    f.app.favorites().delete(favorite.id).unwrap();
    f.jobs.catch_up();

    assert_eq!(f.lists.matches().get(MANGADEX, SCHOLAR).unwrap(), None);
}

#[test]
fn a_favorites_match_is_pruned_once_it_is_removed_even_without_a_list() {
    let f = fixture();
    let favorite = f.favorite(MANGADEX, SCHOLAR, &scholar());
    f.jobs.catch_up();
    assert!(f.lists.matches().get(MANGADEX, SCHOLAR).unwrap().is_some());

    f.app.favorites().delete(favorite.id).unwrap();
    f.jobs.catch_up();

    assert_eq!(f.lists.matches().get(MANGADEX, SCHOLAR).unwrap(), None);
}

#[test]
fn a_link_both_listed_and_a_favorite_is_matched_once_from_the_list() {
    let f = fixture();
    // The favorite's info names nobody who wrote Reborn Rich; the list names its authors.
    f.favorite(
        FIRE,
        "/manga/reborn-rich",
        &FavoriteInfo {
            title: "Reborn Rich".into(),
            authors: "Eiichirou Oda".into(),
            ..FavoriteInfo::default()
        },
    );
    f.lists
        .masterlist()
        .replace_module(
            FIRE,
            [MangaListing {
                authors: "San-Gyeong, JP".into(),
                ..listing("/manga/reborn-rich", "Reborn Rich")
            }],
        )
        .unwrap();

    f.jobs.catch_up();

    assert_eq!(f.series_id(FIRE, "/manga/reborn-rich"), Some(189));
}

fn listing(link: &str, title: &str) -> MangaListing {
    MangaListing {
        link: link.into(),
        title: title.into(),
        ..MangaListing::default()
    }
}

#[test]
fn a_database_refresh_matches_favorites_again() {
    let f = fixture();
    f.favorite(MANGADEX, SCHOLAR, &scholar());
    f.jobs.catch_up();
    assert_eq!(f.mangadex.requests.load(Ordering::SeqCst), 1);

    f.jobs.download().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while f.jobs.is_running() {
        assert!(
            std::time::Instant::now() < deadline,
            "the refresh never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    assert_eq!(f.mangadex.requests.load(Ordering::SeqCst), 2);
    assert_eq!(f.series_id(MANGADEX, SCHOLAR), Some(2723));
}

#[test]
fn a_download_finds_the_match_of_a_favorite_without_a_list() {
    let f = fixture();
    f.favorite(FIRE, "/manga/reborn-rich", &reborn_rich());
    f.jobs.catch_up();

    // The link as stored, and as a module may report it for a download (issue #239).
    for link in [
        "/manga/reborn-rich",
        "https://mangafire.to/manga/reborn-rich",
    ] {
        let found = f.jobs.series_match(FIRE, link).unwrap();
        assert_eq!(found.series.id, 189, "{link}");
    }
}
