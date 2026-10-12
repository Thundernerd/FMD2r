//! MangaBaka series picked by hand (issue #249): they outlive matching, pruning, refreshes and
//! removing the database.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::mangabaka::{FixtureSource, fixture_records};
use fmd_core::metadata::{MangaBakaDb, MangaDexLinks, Matcher, MetadataJobs};
use fmd_core::settings::SettingsService;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_store::{AppDb, ListsDb, MangaListing, MatchConfidence, StoredMatch};

const WEBTOONS: &str = "18f636ec7fdf47fabe95d940ad0b548f";
/// results-71.csv: Blue, `ambiguous` (several series are called Blue; the list names no authors,
/// and MangaBaka has no link to its title_no).
const BLUE: &str = "/en/drama/blue/list?title_no=3477";
/// Reborn Rich, a 2022 manhwa.
const REBORN_RICH: i64 = 189;

/// Never asked: WebToons titles are not looked up on MangaDex.
struct NoNetwork;

impl Transport for NoNetwork {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("no network in tests".into())) })
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    lists: ListsDb,
    source: Arc<FixtureSource>,
    jobs: MetadataJobs,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    let app = AppDb::open(dir.path().join("app.db")).unwrap();
    let source = FixtureSource::new(&fixture_records());
    let db = Arc::new(MangaBakaDb::open(dir.path(), source.clone()));
    let http = HttpClient::with_transport(Arc::new(NoNetwork)).unwrap();
    let jobs = MetadataJobs::new(
        db,
        Matcher::new(lists.clone(), app.clone(), MangaDexLinks::new(http)),
        lists.clone(),
        |id: &str| (id == WEBTOONS).then(|| "https://www.webtoons.com".to_owned()),
        Arc::new(SettingsService::load(app.clone()).unwrap()),
        |_| {},
    );
    Fixture {
        _dir: dir,
        lists,
        source,
        jobs,
    }
}

fn blue() -> MangaListing {
    MangaListing {
        link: BLUE.into(),
        title: "Blue".into(),
        ..MangaListing::default()
    }
}

impl Fixture {
    /// Downloads the database and matches every list against it.
    fn download(&self) {
        self.jobs.download().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.jobs.is_running() {
            assert!(Instant::now() < deadline, "the download did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn get(&self, link: &str) -> Option<StoredMatch> {
        self.lists.matches().get(WEBTOONS, link).unwrap()
    }

    fn assert_reborn_rich_by_hand(&self, link: &str) {
        let m = self.get(link).unwrap();
        assert_eq!(m.confidence, MatchConfidence::Manual);
        assert_eq!(m.series_id, Some(REBORN_RICH));
        assert_eq!(m.format.as_deref(), Some("manhwa"));
        assert_eq!(m.publication.as_deref(), Some("ongoing"));
        assert_eq!(m.year, Some(2022));
    }
}

#[test]
fn a_series_picked_by_hand_replaces_an_ambiguous_match_and_matching_leaves_it_alone() {
    let f = fixture();
    f.lists
        .masterlist()
        .replace_module(WEBTOONS, [blue()])
        .unwrap();
    f.download();
    assert_eq!(f.get(BLUE).unwrap().confidence, MatchConfidence::Ambiguous);

    f.jobs.set_override(WEBTOONS, BLUE, REBORN_RICH).unwrap();
    f.assert_reborn_rich_by_hand(BLUE);

    f.download();
    f.assert_reborn_rich_by_hand(BLUE);
    f.jobs.catch_up();
    f.assert_reborn_rich_by_hand(BLUE);
}

#[test]
fn a_series_picked_by_hand_outside_the_list_and_the_favorites_is_not_pruned() {
    let f = fixture();
    f.lists
        .masterlist()
        .replace_module(WEBTOONS, [blue()])
        .unwrap();
    f.download();
    let unlisted = "/en/drama/reborn-rich/list?title_no=99999";

    f.jobs
        .set_override(WEBTOONS, unlisted, REBORN_RICH)
        .unwrap();
    f.jobs
        .list_changed(WEBTOONS, &fmd_http::TerminateToken::new());

    f.assert_reborn_rich_by_hand(unlisted);
}

#[test]
fn a_series_picked_by_hand_comes_back_after_removing_and_downloading_the_database() {
    let f = fixture();
    let unlisted = "/en/drama/reborn-rich/list?title_no=99999";
    f.download();
    f.jobs
        .set_override(WEBTOONS, unlisted, REBORN_RICH)
        .unwrap();

    f.jobs.remove().unwrap();
    assert_eq!(f.get(unlisted), None);
    f.download();
    f.jobs.catch_up();

    f.assert_reborn_rich_by_hand(unlisted);
}

#[test]
fn removing_a_series_picked_by_hand_matches_the_title_automatically_again() {
    let f = fixture();
    // results-71.csv: Reborn Rich, `link`, 189.
    let reborn_rich = "/en/drama/reborn-rich/list?title_no=4956";
    f.lists
        .masterlist()
        .replace_module(
            WEBTOONS,
            [MangaListing {
                link: reborn_rich.into(),
                title: "Reborn Rich".into(),
                ..MangaListing::default()
            }],
        )
        .unwrap();
    f.download();
    // Shadow Star (results-73.csv).
    f.jobs.set_override(WEBTOONS, reborn_rich, 2092).unwrap();
    assert_eq!(f.get(reborn_rich).unwrap().series_id, Some(2092));

    f.jobs.remove_override(WEBTOONS, reborn_rich).unwrap();

    let m = f.get(reborn_rich).unwrap();
    assert_eq!(
        (m.confidence, m.series_id),
        (MatchConfidence::Link, Some(REBORN_RICH))
    );
    assert_eq!(f.jobs.override_of(WEBTOONS, reborn_rich).unwrap(), None);
}

#[test]
fn a_series_picked_by_hand_that_a_new_build_lacks_stays_stored_but_unused() {
    let f = fixture();
    f.lists
        .masterlist()
        .replace_module(WEBTOONS, [blue()])
        .unwrap();
    f.download();
    f.jobs.set_override(WEBTOONS, BLUE, REBORN_RICH).unwrap();

    let without: Vec<_> = fixture_records()
        .into_iter()
        .filter(|r| r["id"] != REBORN_RICH)
        .collect();
    f.source.set(&without);
    f.download();

    assert!(
        !f.get(BLUE).is_some_and(|m| m.confidence.is_accepted()),
        "{:?}",
        f.get(BLUE)
    );
    assert_eq!(
        f.jobs.override_of(WEBTOONS, BLUE).unwrap(),
        Some(REBORN_RICH)
    );

    // A later build that has it again applies it.
    f.source.set(&fixture_records());
    f.download();
    f.assert_reborn_rich_by_hand(BLUE);
}
