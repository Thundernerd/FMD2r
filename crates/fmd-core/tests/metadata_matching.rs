//! Matching list titles against `metadata.db` (docs/tickets/T73-mangabaka-metadata.md, "Seams
//! under test"). The cases are T71's probe cases (docs/research/metadata-probe/results-*.csv),
//! with the expected series and confidence the probe's final rules gave; the series are the
//! recorded ones in tests/fixtures/mangabaka/series.jsonl. MangaDex's API is a stub.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::mangabaka::{FixtureSource, fixture_records};
use fmd_core::metadata::{ListModule, MangaBakaDb, MangaDexLinks, MatchScope, Matcher};
use fmd_http::{
    BoxFuture, HttpClient, TerminateToken, Transport, TransportError, WireRequest, WireResponse,
};
use fmd_store::{ListsDb, MangaListing, MatchConfidence, StoredMatch};
use serde_json::{Value, json};

const MANGADEX: &str = "d07c9c2425764da8ba056505f57cf40c";
const MANGAFIRE: &str = "23eb3a472201427e8824ecdd5223bad7";
const ASURA: &str = "7103ae6839ea46ec80cdfc2c4b37c803";
const WEBTOONS: &str = "18f636ec7fdf47fabe95d940ad0b548f";

/// MangaDex's `GET /manga?ids[]=...`: answers with the `links` of the titles it knows.
struct MangaDexApi {
    links: Vec<(&'static str, Value)>,
    requests: Mutex<Vec<WireRequest>>,
    /// While set, every request fails as if MangaDex were unreachable.
    down: std::sync::atomic::AtomicBool,
}

impl Transport for MangaDexApi {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        if self.down.load(std::sync::atomic::Ordering::SeqCst) {
            self.requests.lock().unwrap().push(request);
            return Box::pin(async { Err(TransportError("unreachable".into())) });
        }
        let url = decoded(&request.url);
        let data: Vec<Value> = self
            .links
            .iter()
            .filter(|(id, _)| url.contains(&format!("ids[]={id}")))
            .map(|(id, links)| json!({ "id": id, "type": "manga", "attributes": { "links": links } }))
            .collect();
        self.requests.lock().unwrap().push(request);
        let body = json!({ "result": "ok", "response": "collection", "data": data }).to_string();
        let response = WireResponse {
            status: 200,
            reason: "OK".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.into_bytes(),
        };
        Box::pin(async move { Ok(response) })
    }
}

/// `url` with its brackets decoded, as MangaDex reads them.
fn decoded(url: &str) -> String {
    url.replace("%5B", "[").replace("%5D", "]")
}

struct Fixture {
    _dir: tempfile::TempDir,
    lists: ListsDb,
    db: MangaBakaDb,
    matcher: Matcher,
    mangadex: Arc<MangaDexApi>,
}

fn fixture(records: &[Value]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    let db = MangaBakaDb::open(dir.path(), FixtureSource::new(records));
    db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    // The Scholar's Reincarnation's MangaDex links (results-72.csv).
    let mangadex = Arc::new(MangaDexApi {
        links: vec![(
            "f2cb9465-c314-46a3-97fd-05210d3af0a7",
            json!({ "mu": "mxtfjrx", "mal": "147924", "raw": "https://example.com" }),
        )],
        requests: Mutex::default(),
        down: std::sync::atomic::AtomicBool::new(false),
    });
    let http = HttpClient::with_transport(mangadex.clone()).unwrap();
    let matcher = Matcher::new(lists.clone(), MangaDexLinks::new(http));
    Fixture {
        _dir: dir,
        lists,
        db,
        matcher,
        mangadex,
    }
}

fn entry(link: &str, title: &str, alttitles: &str, authors: &str) -> MangaListing {
    MangaListing {
        link: link.into(),
        title: title.into(),
        alttitles: alttitles.into(),
        authors: authors.into(),
        ..MangaListing::default()
    }
}

fn module(id: &str, root_url: &str) -> ListModule {
    ListModule {
        id: id.into(),
        root_url: root_url.into(),
    }
}

impl Fixture {
    /// Lists `entries` as `module`'s list and matches all of it.
    fn match_list(&self, module: &ListModule, entries: &[MangaListing]) -> u64 {
        self.lists
            .masterlist()
            .replace_module(&module.id, entries)
            .unwrap();
        self.run(module, MatchScope::Changed)
    }

    fn run(&self, module: &ListModule, scope: MatchScope) -> u64 {
        let meta = self.db.current().unwrap();
        self.matcher
            .match_module(&meta, module, scope, &TerminateToken::new())
            .unwrap()
            .examined
    }

    fn get(&self, module: &str, link: &str) -> StoredMatch {
        self.lists.matches().get(module, link).unwrap().unwrap()
    }
}

#[test]
fn a_webtoons_title_matches_by_its_title_no() {
    let f = fixture(&fixture_records());
    let webtoons = module(WEBTOONS, "https://www.webtoons.com");
    // results-71.csv: Reborn Rich, `link`, 189.
    f.match_list(
        &webtoons,
        &[entry(
            "/en/drama/reborn-rich/list?title_no=4956",
            "Reborn Rich",
            "",
            "",
        )],
    );

    let m = f.get(WEBTOONS, "/en/drama/reborn-rich/list?title_no=4956");
    assert_eq!(m.confidence, MatchConfidence::Link);
    assert_eq!(m.series_id, Some(189));
    assert_eq!(m.format.as_deref(), Some("manhwa"));
    assert_eq!(m.publication.as_deref(), Some("ongoing"));
    assert_eq!(m.year, Some(2022));
}

#[test]
fn a_mangadex_title_matches_by_its_cross_site_ids() {
    let f = fixture(&fixture_records());
    let mangadex = module(MANGADEX, "https://mangadex.org");
    // results-72.csv: The Scholar's Reincarnation, `cross-id`, 2723 ("Reborn as a Scholar").
    f.match_list(
        &mangadex,
        &[entry(
            "/title/f2cb9465-c314-46a3-97fd-05210d3af0a7",
            "The Scholar's Reincarnation",
            "",
            "Yu Hyun So (소유현)",
        )],
    );

    let m = f.get(MANGADEX, "/title/f2cb9465-c314-46a3-97fd-05210d3af0a7");
    assert_eq!(m.confidence, MatchConfidence::CrossId);
    assert_eq!(m.series_id, Some(2723));
    let requests = f.mangadex.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "one request for up to 100 titles");
    let url = &decoded(&requests[0].url);
    assert!(url.starts_with("https://api.mangadex.org/manga?"), "{url}");
    assert!(
        url.contains("ids[]=f2cb9465-c314-46a3-97fd-05210d3af0a7"),
        "{url}"
    );
    assert!(url.contains("limit=100"), "{url}");
    let agent = requests[0]
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert!(agent.starts_with("FMD2r/"), "{agent}");
}

#[test]
fn mangadex_is_asked_in_batches_of_100() {
    let f = fixture(&fixture_records());
    let mangadex = module(MANGADEX, "https://mangadex.org");
    let entries: Vec<MangaListing> = (0..150)
        .map(|i| {
            entry(
                &format!("/title/00000000-0000-0000-0000-{i:012}"),
                &format!("Unknown title {i}"),
                "",
                "",
            )
        })
        .collect();
    f.match_list(&mangadex, &entries);

    let requests = f.mangadex.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(decoded(&requests[0].url).matches("ids[]=").count(), 100);
    assert_eq!(decoded(&requests[1].url).matches("ids[]=").count(), 50);
}

#[test]
fn authors_match_despite_romanisation_differences() {
    let f = fixture(&fixture_records());
    let mangafire = module(MANGAFIRE, "https://mangafire.to");
    let asura = module(ASURA, "https://asurascans.com");
    // results-73.csv: Shadow Star☆ by Mohiro Kitoh, `title+author`, 2092 (Mohiro Kitou).
    f.match_list(
        &mangafire,
        &[entry(
            "/manga/narutaruu.pj6q",
            "Shadow Star☆",
            "",
            "Mohiro Kitoh",
        )],
    );
    // results-72.csv: Revenge of the Iron-Blooded Sword Hound by Legobalbasseo,
    // `title+author`, 808 (Lego Balbasseo).
    f.match_list(
        &asura,
        &[entry(
            "/comics/revenge-of-the-iron-blooded-sword-hound",
            "Revenge of the Iron-Blooded Sword Hound",
            "",
            "Legobalbasseo",
        )],
    );

    let m = f.get(MANGAFIRE, "/manga/narutaruu.pj6q");
    assert_eq!(
        (m.confidence, m.series_id),
        (MatchConfidence::TitleAuthor, Some(2092))
    );
    let m = f.get(ASURA, "/comics/revenge-of-the-iron-blooded-sword-hound");
    assert_eq!(
        (m.confidence, m.series_id),
        (MatchConfidence::TitleAuthor, Some(808))
    );
}

#[test]
fn decorations_are_stripped_from_titles() {
    let f = fixture(&fixture_records());
    let mangafire = module(MANGAFIRE, "https://mangafire.to");
    // results-71.csv: The Girl Who Talks Too Much (Colored) by Juukyuu, `title+author`, 74312.
    f.match_list(
        &mangafire,
        &[entry(
            "/manga/the-girl-who-talks-too-muchh.lrk3q",
            "The Girl Who Talks Too Much (Colored)",
            "",
            "Juukyuu",
        )],
    );

    let m = f.get(MANGAFIRE, "/manga/the-girl-who-talks-too-muchh.lrk3q");
    assert_eq!(
        (m.confidence, m.series_id),
        (MatchConfidence::TitleAuthor, Some(74312))
    );
}

#[test]
fn a_title_whose_authors_disagree_is_rejected() {
    let f = fixture(&fixture_records());
    let asura = module(ASURA, "https://asurascans.com");
    // results-71.csv: Never Die Extra by Eldo, `author-conflict` (the comic lists its adapters,
    // the pre-serialization edition Toika).
    f.match_list(
        &asura,
        &[entry(
            "/comics/never-die-extra",
            "Never Die Extra",
            "",
            "Eldo",
        )],
    );

    let m = f.get(ASURA, "/comics/never-die-extra");
    assert_eq!(m.confidence, MatchConfidence::AuthorConflict);
    assert_eq!(m.series_id, None);
    assert_eq!(m.format, None);
}

#[test]
fn a_novel_is_never_the_match() {
    let f = fixture(&fixture_records());
    let asura = module(ASURA, "https://asurascans.com");
    // results-72-round1.csv: the first rules matched "Starting Today, I’m a Player" by Gavinge
    // to the novel 83451 ("From Today, I'm a Player", by GaVinGe); the final rules drop novels.
    f.match_list(
        &asura,
        &[entry(
            "/comics/starting-today-im-a-player",
            "Starting Today, I’m a Player",
            "From Today, I'm a Player",
            "Gavinge",
        )],
    );

    let m = f.get(ASURA, "/comics/starting-today-im-a-player");
    assert!(!m.confidence.is_accepted(), "{m:?}");
    assert_eq!(m.series_id, None);
}

#[test]
fn a_title_several_series_share_is_ambiguous() {
    let f = fixture(&fixture_records());
    let webtoons = module(WEBTOONS, "https://www.webtoons.com");
    // results-71.csv: Blue, `ambiguous` (several series are called Blue; the list names no
    // authors, and MangaBaka has no link to its title_no).
    f.match_list(
        &webtoons,
        &[entry("/en/drama/blue/list?title_no=3477", "Blue", "", "")],
    );

    let m = f.get(WEBTOONS, "/en/drama/blue/list?title_no=3477");
    assert_eq!(m.confidence, MatchConfidence::Ambiguous);
    assert_eq!(m.series_id, None);
}

#[test]
fn a_title_more_than_50_series_share_is_ambiguous_even_with_authors() {
    let mut records = fixture_records();
    let template = records[0].clone();
    for i in 0..51 {
        let mut r = template.clone();
        r["id"] = json!(900_000 + i);
        r["title"] = json!("Love");
        r["titles"] = json!([]);
        r["secondary_titles"] = json!({});
        r["native_title"] = Value::Null;
        r["romanized_title"] = Value::Null;
        r["links"] = json!([]);
        r["source"] = json!({});
        r["authors"] = json!([format!("Author {i}")]);
        records.push(r);
    }
    let f = fixture(&records);
    let mangafire = module(MANGAFIRE, "https://mangafire.to");
    f.match_list(&mangafire, &[entry("/manga/love", "Love", "", "Author 7")]);

    let m = f.get(MANGAFIRE, "/manga/love");
    assert_eq!(m.confidence, MatchConfidence::Ambiguous);
}

#[test]
fn a_list_update_rematches_only_new_or_retitled_titles_and_a_refresh_all() {
    let f = fixture(&fixture_records());
    let mangafire = module(MANGAFIRE, "https://mangafire.to");
    let shadow = entry("/manga/narutaruu.pj6q", "Shadow Star☆", "", "Mohiro Kitoh");
    let girl = entry(
        "/manga/the-girl-who-talks-too-muchh.lrk3q",
        "The Girl Who Talks Too Much (Colored)",
        "",
        "Juukyuu",
    );
    let blue = entry("/manga/blue", "Blue", "", "Kiriko Nananan");
    assert_eq!(f.match_list(&mangafire, &[shadow.clone(), girl.clone()]), 2);
    assert_eq!(f.run(&mangafire, MatchScope::Changed), 0, "nothing changed");

    // An update adds Blue and retitles Shadow Star☆; The Girl Who Talks Too Much stays.
    let retitled = MangaListing {
        title: "Narutaru".into(),
        ..shadow.clone()
    };
    f.lists.masterlist().upsert(MANGAFIRE, &retitled).unwrap();
    f.lists.masterlist().insert_new(MANGAFIRE, [&blue]).unwrap();
    assert_eq!(f.run(&mangafire, MatchScope::Changed), 2);
    let m = f.get(MANGAFIRE, "/manga/blue");
    assert_eq!(
        (m.confidence, m.series_id),
        (MatchConfidence::TitleAuthor, Some(7114))
    );
    assert_eq!(f.get(MANGAFIRE, &shadow.link).series_id, Some(2092));

    // A refresh: every title again.
    assert_eq!(f.run(&mangafire, MatchScope::All), 3);
}

#[test]
fn matches_of_titles_no_longer_listed_are_dropped() {
    let f = fixture(&fixture_records());
    let mangafire = module(MANGAFIRE, "https://mangafire.to");
    let shadow = entry("/manga/narutaruu.pj6q", "Shadow Star☆", "", "Mohiro Kitoh");
    let blue = entry("/manga/blue", "Blue", "", "Kiriko Nananan");
    f.match_list(&mangafire, &[shadow.clone(), blue.clone()]);

    f.match_list(&mangafire, &[shadow]);

    assert!(
        f.lists
            .matches()
            .get(MANGAFIRE, "/manga/blue")
            .unwrap()
            .is_none()
    );
}

#[test]
fn titles_matched_against_an_older_database_count_as_changed() {
    let f = fixture(&fixture_records());
    let mangafire = module(MANGAFIRE, "https://mangafire.to");
    let shadow = entry("/manga/narutaruu.pj6q", "Shadow Star☆", "", "Mohiro Kitoh");
    let blue = entry("/manga/blue", "Blue", "", "Kiriko Nananan");
    f.match_list(&mangafire, &[shadow, blue]);

    // A refresh whose matching was cancelled: the next run still owes every title.
    f.db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
    assert_eq!(f.run(&mangafire, MatchScope::Changed), 2);
    assert_eq!(f.run(&mangafire, MatchScope::Changed), 0);
}

#[test]
fn titles_matched_while_mangadex_was_unreachable_are_asked_again() {
    let f = fixture(&fixture_records());
    let mangadex = module(MANGADEX, "https://mangadex.org");
    let link = "/title/f2cb9465-c314-46a3-97fd-05210d3af0a7";
    f.mangadex
        .down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    f.match_list(
        &mangadex,
        &[entry(
            link,
            "The Scholar's Reincarnation",
            "",
            "Yu Hyun So (소유현)",
        )],
    );
    assert_ne!(f.get(MANGADEX, link).confidence, MatchConfidence::CrossId);

    f.mangadex
        .down
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(f.run(&mangadex, MatchScope::Changed), 1);
    assert_eq!(f.get(MANGADEX, link).confidence, MatchConfidence::CrossId);
}
