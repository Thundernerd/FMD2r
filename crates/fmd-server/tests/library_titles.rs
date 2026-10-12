//! The title the Library shows (issue #238, "Seams under test"): `GET /api/favorites` over
//! fmd-core's recorded MangaBaka dump (crates/fmd-core/tests/fixtures/mangabaka) and a list
//! containing the favorites' links.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fmd_core::metadata::{
    Download, DumpSource, MangaBakaDb, MangaDexLinks, Matcher, MetadataError, MetadataJobs,
};
use fmd_core::modules::ModuleInfo;
use fmd_http::{
    BoxFuture, HttpClient, TerminateToken, Transport, TransportError, WireRequest, WireResponse,
};
use fmd_server::{AppState, ModuleCatalog, ModulesReport, build_router};
use fmd_store::{
    AppDb, FavoriteInfo, ListsDb, MangaListing, MatchConfidence, MatchInput, NewFavorite,
    StoredMatch,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// The recorded dump, compressed as MangaBaka serves it.
struct FixtureDump;

impl DumpSource for FixtureDump {
    fn open(&self, _url: &str, _terminate: &TerminateToken) -> Result<Download, MetadataError> {
        let jsonl = std::fs::read(format!(
            "{}/../fmd-core/tests/fixtures/mangabaka/series.jsonl",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let body = zstd::encode_all(jsonl.as_slice(), 3).unwrap();
        let length = Some(body.len() as u64);
        let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(body));
        Ok(Download { reader, length })
    }
}

struct NoNetwork;

impl Transport for NoNetwork {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("no network in tests".into())) })
    }
}

struct Modules;

impl ModuleCatalog for Modules {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        vec![ModuleInfo {
            id: "fire".into(),
            name: "MangaFire".into(),
            root_url: "https://mangafire.to".into(),
            category: "English".into(),
            limits: Default::default(),
            options: Vec::new(),
            capabilities: Default::default(),
        }]
    }
}

/// The favorites, as `(link, website title, match)`, in the order the Library lists them.
/// The fixture's series 2092 is "Shadow Star☆" (English "Shadow Star☆", romanized "Mukuronaru
/// hoshi", native "なるたる"); 83451 "From Today, I'm a Player" has no romanized title; 952 is
/// deleted.
fn favorites() -> Vec<(&'static str, &'static str, Option<StoredMatch>)> {
    vec![
        ("/narutaru", "Narutaru", Some(accepted(2092))),
        ("/player", "Player From Today", Some(accepted(83451))),
        ("/princesses", "Failed Princess", Some(accepted(952))),
        (
            "/rejected",
            "One Piece",
            Some(StoredMatch {
                series_id: None,
                confidence: MatchConfidence::Ambiguous,
                format: None,
                publication: None,
                year: None,
            }),
        ),
        ("/unmatched", "A Title Nobody Has", None),
    ]
}

fn accepted(id: i64) -> StoredMatch {
    StoredMatch {
        series_id: Some(id),
        confidence: MatchConfidence::TitleUnique,
        format: Some("manga".into()),
        publication: None,
        year: None,
    }
}

/// `db` and `lists` served with MangaBaka's database built from the fixture.
fn with_mangabaka(dir: &TempDir, db: AppDb, lists: ListsDb) -> (AppState, MetadataJobs) {
    let mangabaka = Arc::new(MangaBakaDb::open(dir.path(), Arc::new(FixtureDump)));
    mangabaka
        .refresh(&TerminateToken::new(), &mut |_| {})
        .unwrap();
    let state = AppState::new(db.clone())
        .unwrap()
        .with_modules(Modules)
        .with_lists(lists.clone());
    let http = HttpClient::with_transport(Arc::new(NoNetwork)).unwrap();
    let jobs = MetadataJobs::new(
        mangabaka,
        Matcher::new(lists.clone(), db, MangaDexLinks::new(http)),
        lists,
        |id: &str| (id == "fire").then(|| "https://mangafire.to".to_owned()),
        state.settings().clone(),
        state.metadata_events(),
    );
    (state.with_metadata(jobs.clone()), jobs)
}

struct Library {
    _dir: TempDir,
    state: AppState,
}

impl Library {
    /// With MangaBaka's database built and the favorites' list matched.
    fn new() -> Library {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
        let favorites = favorites();
        lists
            .masterlist()
            .replace_module(
                "fire",
                favorites.iter().map(|(link, title, _)| MangaListing {
                    link: (*link).into(),
                    title: (*title).into(),
                    ..MangaListing::default()
                }),
            )
            .unwrap();
        let inputs = lists.matches().all("fire", "test").unwrap();
        let decided: Vec<(MatchInput, StoredMatch)> = inputs
            .into_iter()
            .filter_map(|input| {
                let (_, _, m) = favorites.iter().find(|(l, _, _)| *l == input.link)?;
                Some((input, m.clone()?))
            })
            .collect();
        lists
            .matches()
            .store("fire", decided.iter().map(|(i, m)| (i, m)))
            .unwrap();
        for (link, title, _) in &favorites {
            db.favorites()
                .create(&NewFavorite {
                    module_id: "fire".into(),
                    link: (*link).into(),
                    title: (*title).into(),
                    save_to: format!("downloads/{title}"),
                    cover_url: None,
                })
                .unwrap();
        }

        let (state, _) = with_mangabaka(&dir, db, lists);
        Library { _dir: dir, state }
    }

    fn show(&self, library_title: &str) {
        self.state
            .settings()
            .update(json!({ "appearance": { "library_title": library_title } }))
            .unwrap();
    }

    async fn get(&self, uri: &str) -> Vec<Value> {
        let res = build_router(self.state.clone())
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// `(title, display_title)` of each favorite listed.
    async fn titles(&self, uri: &str) -> Vec<(String, String)> {
        self.get(uri)
            .await
            .iter()
            .map(|f| {
                (
                    f["title"].as_str().unwrap().to_owned(),
                    f["display_title"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[tokio::test]
async fn english_shows_mangabakas_english_title_and_keeps_the_websites_as_title() {
    let library = Library::new();
    library.show("english");

    assert_eq!(
        library.titles("/api/favorites").await,
        pairs(&[
            ("Narutaru", "Shadow Star☆"),
            ("Player From Today", "From Today, I'm a Player"),
            ("Failed Princess", "Failed Princess"),
            ("One Piece", "One Piece"),
            ("A Title Nobody Has", "A Title Nobody Has"),
        ])
    );
}

#[tokio::test]
async fn native_shows_mangabakas_native_title() {
    let library = Library::new();
    library.show("native");

    let titles = library.titles("/api/favorites").await;
    assert_eq!(
        titles[..2],
        pairs(&[
            ("Narutaru", "なるたる"),
            ("Player From Today", "오늘부터 플레이어"),
        ])
    );
}

#[tokio::test]
async fn website_shows_the_websites_title() {
    let library = Library::new();
    library.show("website");

    assert_eq!(
        library.titles("/api/favorites").await,
        pairs(&[
            ("Narutaru", "Narutaru"),
            ("Player From Today", "Player From Today"),
            ("Failed Princess", "Failed Princess"),
            ("One Piece", "One Piece"),
            ("A Title Nobody Has", "A Title Nobody Has"),
        ])
    );
}

#[tokio::test]
async fn a_series_without_the_chosen_title_shows_mangabakas_main_title() {
    let library = Library::new();
    library.show("romanized");

    assert_eq!(
        library.titles("/api/favorites").await,
        pairs(&[
            ("Narutaru", "Mukuronaru hoshi"),
            // MangaBaka has no romanized title for it.
            ("Player From Today", "From Today, I'm a Player"),
            // Matched to a series MangaBaka has since deleted.
            ("Failed Princess", "Failed Princess"),
            // Rejected as ambiguous.
            ("One Piece", "One Piece"),
            // Never matched.
            ("A Title Nobody Has", "A Title Nobody Has"),
        ])
    );
}

#[tokio::test]
async fn mangabaka_shows_mangabakas_main_title() {
    let library = Library::new();
    library.show("mangabaka");

    let titles = library.titles("/api/favorites").await;
    assert_eq!(
        titles[..2],
        pairs(&[
            ("Narutaru", "Shadow Star☆"),
            ("Player From Today", "From Today, I'm a Player"),
        ])
    );
}

#[tokio::test]
async fn the_search_matches_the_shown_and_the_websites_title() {
    let library = Library::new();
    library.show("native");

    let shown = library.titles("/api/favorites?q=%E3%81%AA%E3%82%8B").await;
    assert_eq!(shown, pairs(&[("Narutaru", "なるたる")]));
    let website = library.titles("/api/favorites?q=narutaru").await;
    assert_eq!(website, pairs(&[("Narutaru", "なるたる")]));
    let unmatched = library.titles("/api/favorites?q=nobody").await;
    assert_eq!(
        unmatched,
        pairs(&[("A Title Nobody Has", "A Title Nobody Has")])
    );
}

#[tokio::test]
async fn a_changed_title_shows_in_the_patch_response() {
    let library = Library::new();
    library.show("english");
    let id = library.get("/api/favorites").await[0]["id"]
        .as_i64()
        .unwrap();

    let res = build_router(library.state.clone())
        .oneshot(
            Request::patch(format!("/api/favorites/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "enabled": false }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["title"], "Narutaru");
    assert_eq!(body["display_title"], "Shadow Star☆");
}

#[tokio::test]
async fn without_mangabakas_database_the_websites_title_shows() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    db.favorites()
        .create(&NewFavorite {
            module_id: "fire".into(),
            link: "/narutaru".into(),
            title: "Narutaru".into(),
            save_to: "downloads/Narutaru".into(),
            cover_url: None,
        })
        .unwrap();
    let library = Library {
        state: AppState::new(db).unwrap().with_modules(Modules),
        _dir: dir,
    };
    library.show("english");

    assert_eq!(
        library.titles("/api/favorites").await,
        pairs(&[("Narutaru", "Narutaru")])
    );
}

#[tokio::test]
async fn a_favorite_matched_without_its_websites_list_shows_the_chosen_title() {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    // The list of `fire` was never downloaded (issue #242).
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    let favorite = db
        .favorites()
        .create(&NewFavorite {
            module_id: "fire".into(),
            link: "/reborn-rich".into(),
            title: "Reborn Rich".into(),
            save_to: "downloads/Reborn Rich".into(),
            cover_url: None,
        })
        .unwrap();
    db.favorites()
        .set_info(
            favorite.id,
            &FavoriteInfo {
                title: "Reborn Rich".into(),
                authors: "San-Gyeong, JP".into(),
                ..FavoriteInfo::default()
            },
        )
        .unwrap();
    let (state, jobs) = with_mangabaka(&dir, db, lists);
    tokio::task::spawn_blocking(move || jobs.catch_up())
        .await
        .unwrap();
    let library = Library { _dir: dir, state };
    library.show("native");

    // Series 189's native title in the fixture.
    assert_eq!(
        library.titles("/api/favorites").await,
        pairs(&[("Reborn Rich", "재벌집 막내아들")])
    );
}
