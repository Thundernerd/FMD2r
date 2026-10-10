//! `GET /api/covers/series`: a list title's cover, from its stored link, its MangaBaka match or
//! its website's `GetInfo` (docs/tickets/T70-discover-cover-thumbnails.md, "Seams under test").
//! The websites and MangaBaka's CDN are a mocked network; the MangaBaka database is built from
//! fmd-core's recorded fixture (crates/fmd-core/tests/fixtures/mangabaka).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::info::{InfoError, InfoOptions, MangaInfo};
use fmd_core::metadata::{
    Download, DumpSource, MangaBakaDb, MangaDexLinks, Matcher, MetadataError, MetadataJobs,
};
use fmd_core::modules::ModuleInfo;
use fmd_core::settings::ModuleLimits;
use fmd_http::{
    BoxFuture, HttpClient, HttpSession, ModuleHttp, TerminateToken, Transport, TransportError,
    WireRequest, WireResponse,
};
use fmd_server::{
    AppState, CoverConfig, CoverModules, CoverResolver, CoverSession, ModuleCatalog, ModulesReport,
    build_router,
};
use fmd_store::{AppDb, ListsDb, MangaListing, MatchConfidence, MatchInput, StoredMatch};
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

const MODULE_UA: &str = "CoverTest/1.0";

/// A 400x300 PNG.
fn png() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(400, 300, image::Rgb([200, 30, 30]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// Every host resolves to one public address.
struct PublicDns;

impl CoverResolver for PublicDns {
    fn resolve(&self, _host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        Ok(vec![SocketAddr::from(([93, 184, 216, 34], port))])
    }
}

/// The network: every URL is a PNG. Records the requests.
#[derive(Default)]
struct Network {
    requests: Mutex<Vec<WireRequest>>,
}

impl Network {
    fn urls(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.url.clone())
            .collect()
    }

    fn header(&self, url: &str, name: &str) -> Option<String> {
        let requests = self.requests.lock().unwrap();
        let request = requests.iter().find(|r| r.url == url).unwrap();
        request
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
}

impl Transport for Network {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.requests.lock().unwrap().push(request);
        Box::pin(async {
            Ok(WireResponse {
                status: 200,
                reason: String::new(),
                headers: vec![("Content-Type".into(), "image/png".into())],
                body: png(),
            })
        })
    }
}

/// One website module, `site` on `http://site.test`, whose `GetInfo` gives each link the cover
/// in `covers` (none when it is missing there). It counts the calls, and the most at once.
struct Site {
    client: HttpClient,
    http: ModuleHttp,
    covers: HashMap<String, String>,
    max_connection_limit: u32,
    /// How long one `GetInfo` takes.
    delay: Duration,
    calls: AtomicUsize,
    /// Whether the website is down: `GetInfo` fails with a network problem.
    down: std::sync::atomic::AtomicBool,
    in_flight: AtomicUsize,
    most_in_flight: AtomicUsize,
}

impl Site {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

/// The site's modules and covers, shared by the state's catalog and cover sessions.
#[derive(Clone)]
struct Shared(Arc<Site>);

impl ModuleCatalog for Shared {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        vec![ModuleInfo {
            id: "site".into(),
            name: "Site".into(),
            root_url: "http://site.test".into(),
            category: "English".into(),
            limits: ModuleLimits {
                max_connection_limit: self.0.max_connection_limit,
                ..ModuleLimits::default()
            },
            options: Vec::new(),
            capabilities: Default::default(),
        }]
    }

    fn get_info(
        &self,
        id: &str,
        link: &str,
        _options: InfoOptions,
    ) -> BoxFuture<'static, Result<MangaInfo, InfoError>> {
        let site = self.0.clone();
        let known = id == "site";
        let link = link.to_owned();
        Box::pin(async move {
            if !known {
                return Err(InfoError::UnknownModule);
            }
            site.calls.fetch_add(1, Ordering::SeqCst);
            if site.down.load(Ordering::SeqCst) {
                return Err(InfoError::NetProblem);
            }
            let now = site.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            site.most_in_flight.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(site.delay).await;
            site.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(MangaInfo {
                title: link.clone(),
                cover_link: site.covers.get(&link).cloned().unwrap_or_default(),
                link,
                ..MangaInfo::default()
            })
        })
    }
}

impl CoverModules for Shared {
    fn cover_session(&self, id: &str) -> Option<CoverSession> {
        (id == "site").then(|| {
            let mut session = self.0.client.session_for(&self.0.http);
            session.set_user_agent(MODULE_UA);
            CoverSession {
                root_url: "http://site.test".into(),
                session,
            }
        })
    }

    fn plain_session(&self) -> Option<HttpSession> {
        Some(self.0.client.session())
    }
}

/// The recorded MangaBaka dump, compressed as MangaBaka serves it.
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

/// Nothing asks MangaDex here.
struct NoNetwork;

impl Transport for NoNetwork {
    fn send(&self, _: WireRequest) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        Box::pin(async { Err(TransportError("no network in tests".into())) })
    }
}

struct Harness {
    dir: TempDir,
    site: Arc<Site>,
    network: Arc<Network>,
    lists: ListsDb,
}

const LINKS: [&str; 4] = ["/shadow", "/baskerville", "/onepiece", "/nocover"];

fn harness(covers: &[(&str, &str)], max_connection_limit: u32, delay: Duration) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let network = Arc::new(Network::default());
    let client = HttpClient::with_transport(network.clone()).unwrap();
    let http = client.module("site");
    let site = Arc::new(Site {
        client,
        http,
        covers: covers
            .iter()
            .map(|(l, c)| (l.to_string(), c.to_string()))
            .collect(),
        max_connection_limit,
        delay,
        calls: AtomicUsize::new(0),
        down: Default::default(),
        in_flight: AtomicUsize::new(0),
        most_in_flight: AtomicUsize::new(0),
    });
    let lists = ListsDb::open(dir.path().join("lists.db")).unwrap();
    lists
        .masterlist()
        .replace_module(
            "site",
            LINKS.iter().map(|link| MangaListing {
                link: link.to_string(),
                title: link.to_string(),
                ..MangaListing::default()
            }),
        )
        .unwrap();
    Harness {
        dir,
        site,
        network,
        lists,
    }
}

impl Harness {
    /// A server over the harness's data, as after a restart: nothing is kept in memory.
    fn state(&self) -> AppState {
        self.state_with(|_| {})
    }

    fn state_with(&self, config: impl FnOnce(&mut CoverConfig)) -> AppState {
        let db = AppDb::open(self.dir.path().join("app.db")).unwrap();
        let mut covers = CoverConfig::new(self.dir.path().join("covers"));
        covers.resolver = Arc::new(PublicDns);
        config(&mut covers);
        AppState::new(db)
            .unwrap()
            .with_modules(Shared(self.site.clone()))
            .with_covers(covers, Shared(self.site.clone()))
            .with_lists(self.lists.clone())
    }

    /// [`Harness::state`] with the MangaBaka database built from the fixture, and matches for
    /// the list: Shadow Star☆ and Baskerville accepted, One Piece rejected as ambiguous.
    fn state_with_mangabaka(&self) -> AppState {
        let state = self.state();
        let db = Arc::new(MangaBakaDb::open(self.dir.path(), Arc::new(FixtureDump)));
        db.refresh(&TerminateToken::new(), &mut |_| {}).unwrap();
        let http = HttpClient::with_transport(Arc::new(NoNetwork)).unwrap();
        let jobs = MetadataJobs::new(
            db,
            Matcher::new(self.lists.clone(), MangaDexLinks::new(http)),
            self.lists.clone(),
            |id: &str| (id == "site").then(|| "http://site.test".to_owned()),
            state.settings().clone(),
            state.metadata_events(),
        );
        store_matches(&self.lists);
        state.with_metadata(jobs)
    }
}

fn store_matches(lists: &ListsDb) {
    let inputs = lists.matches().all("site", "test").unwrap();
    let decided: Vec<(MatchInput, StoredMatch)> = inputs
        .into_iter()
        .filter_map(|input| {
            let (series_id, confidence) = match input.link.as_str() {
                "/shadow" => (Some(2092), MatchConfidence::TitleAuthor),
                "/baskerville" => (Some(808), MatchConfidence::TitleUnique),
                "/onepiece" => (None, MatchConfidence::Ambiguous),
                _ => return None,
            };
            let m = StoredMatch {
                series_id,
                confidence,
                format: None,
                publication: None,
                year: None,
            };
            Some((input, m))
        })
        .collect();
    lists
        .matches()
        .store("site", decided.iter().map(|(i, m)| (i, m)))
        .unwrap();
}

fn series_cover(link: &str, w: Option<u32>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("module", "site")
        .append_pair("link", link);
    if let Some(w) = w {
        query.append_pair("w", &w.to_string());
    }
    format!("/api/covers/series?{}", query.finish())
}

async fn send(state: &AppState, uri: &str) -> Response {
    build_router(state.clone())
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn image(state: &AppState, uri: &str) -> Vec<u8> {
    let res = send(state, uri).await;
    assert_eq!(res.status(), StatusCode::OK, "{uri}");
    res.into_body().collect().await.unwrap().to_bytes().to_vec()
}

fn quick() -> Duration {
    Duration::from_millis(1)
}

#[tokio::test(flavor = "multi_thread")]
async fn get_info_runs_once_per_title_and_later_calls_use_the_stored_link() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );

    let state = h.state();
    assert_eq!(image(&state, &series_cover("/shadow", None)).await, png());
    assert_eq!(h.site.calls(), 1);
    assert_eq!(h.network.urls(), ["http://site.test/covers/shadow.png"]);
    // Fetched with the module's session.
    assert_eq!(
        h.network
            .header("http://site.test/covers/shadow.png", "User-Agent")
            .as_deref(),
        Some(MODULE_UA)
    );

    // After a restart, the link is still known.
    let state = h.state();
    assert!(
        !image(&state, &series_cover("/shadow", Some(100)))
            .await
            .is_empty()
    );
    assert_eq!(h.site.calls(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_title_without_a_cover_is_a_404_without_asking_the_website_again() {
    let h = harness(&[], 0, quick());
    let state = h.state();

    let res = send(&state, &series_cover("/nocover", None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(h.site.calls(), 1);

    let state = h.state();
    let res = tokio::time::timeout(
        Duration::from_secs(1),
        send(&state, &series_cover("/nocover", None)),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(h.site.calls(), 1);
    assert!(h.network.urls().is_empty());
}

/// Asks for the covers of `count` different titles of `site` at once; returns how many were
/// served.
async fn many_at_once(state: &AppState, count: usize) -> usize {
    let requests = (0..count).map(|i| {
        let state = state.clone();
        tokio::spawn(async move {
            send(&state, &series_cover(&format!("/title-{i}"), None))
                .await
                .status()
        })
    });
    let mut served = 0;
    for request in requests.collect::<Vec<_>>() {
        if request.await.unwrap() == StatusCode::OK {
            served += 1;
        }
    }
    served
}

fn titled(count: usize) -> Vec<(String, String)> {
    (0..count)
        .map(|i| {
            (
                format!("/title-{i}"),
                format!("http://site.test/covers/{i}.png"),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_few_get_info_lookups_run_at_once_per_module() {
    let covers = titled(12);
    let covers: Vec<(&str, &str)> = covers.iter().map(|(l, c)| (&**l, &**c)).collect();
    let h = harness(&covers, 0, Duration::from_millis(50));

    assert_eq!(many_at_once(&h.state(), 12).await, 12);
    assert_eq!(h.site.calls(), 12);
    let most = h.site.most_in_flight.load(Ordering::SeqCst);
    assert!((2..=3).contains(&most), "{most} at once");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_module_connection_limit_lowers_the_lookups_at_once() {
    let covers = titled(6);
    let covers: Vec<(&str, &str)> = covers.iter().map(|(l, c)| (&**l, &**c)).collect();
    let h = harness(&covers, 1, Duration::from_millis(20));

    assert_eq!(many_at_once(&h.state(), 6).await, 6);
    assert_eq!(h.site.calls(), 6);
    assert_eq!(h.site.most_in_flight.load(Ordering::SeqCst), 1);
}

const SHADOW_X250: &str = "https://cdn.mangabaka.dev/imgproxy/plain/x250@1/aHR0cHM6Ly9zNC5hbmlsaXN0LmNvL2ZpbGUvYW5pbGlzdGNkbi9tZWRpYS9tYW5nYS9jb3Zlci9sYXJnZS9ieDMxMTUzLWJzNHRYSHRyR1gzYS5qcGc";
const SHADOW_X350: &str = "https://cdn.mangabaka.dev/imgproxy/plain/x350@1/aHR0cHM6Ly9zNC5hbmlsaXN0LmNvL2ZpbGUvYW5pbGlzdGNkbi9tZWRpYS9tYW5nYS9jb3Zlci9sYXJnZS9ieDMxMTUzLWJzNHRYSHRyR1gzYS5qcGc";

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_mangabaka_match_serves_its_thumbnail_without_get_info() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    let state = h.state_with_mangabaka();

    assert!(
        !image(&state, &series_cover("/shadow", Some(150)))
            .await
            .is_empty()
    );
    assert!(
        !image(&state, &series_cover("/shadow", Some(300)))
            .await
            .is_empty()
    );

    assert_eq!(h.site.calls(), 0);
    assert_eq!(h.network.urls(), [SHADOW_X250, SHADOW_X350]);
    // Not with the module's session: no module user agent, cookies or referer.
    assert_ne!(
        h.network.header(SHADOW_X250, "User-Agent").as_deref(),
        Some(MODULE_UA)
    );
    assert_eq!(h.network.header(SHADOW_X250, "Referer"), None);

    // The link is stored too.
    let state = h.state_with_mangabaka();
    assert!(
        !image(&state, &series_cover("/shadow", Some(150)))
            .await
            .is_empty()
    );
    assert_eq!(h.site.calls(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_title_without_an_accepted_match_falls_back_to_get_info() {
    let h = harness(
        &[
            ("/onepiece", "http://site.test/covers/onepiece.png"),
            ("/nocover", "http://site.test/covers/nomatch.png"),
        ],
        0,
        quick(),
    );
    let state = h.state_with_mangabaka();

    // Rejected as ambiguous.
    assert_eq!(image(&state, &series_cover("/onepiece", None)).await, png());
    // Not matched at all.
    assert_eq!(image(&state, &series_cover("/nocover", None)).await, png());

    assert_eq!(h.site.calls(), 2);
    assert_eq!(
        h.network.urls(),
        [
            "http://site.test/covers/onepiece.png",
            "http://site.test/covers/nomatch.png"
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_database_every_title_goes_to_get_info() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    // The matches are stored, but the database they point into is not there.
    store_matches(&h.lists);
    let state = h.state();

    assert_eq!(image(&state, &series_cover("/shadow", None)).await, png());
    assert_eq!(h.site.calls(), 1);
    assert_eq!(h.network.urls(), ["http://site.test/covers/shadow.png"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_covers_off_nothing_is_looked_up_or_fetched() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    let state = h.state_with_mangabaka();
    state
        .settings()
        .update(serde_json::json!({ "general": { "load_covers": false } }))
        .unwrap();

    for link in ["/shadow", "/onepiece"] {
        let res = send(&state, &series_cover(link, None)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(h.site.calls(), 0);
    assert!(h.network.urls().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_match_a_refresh_drops_replaces_the_stored_mangabaka_link() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    let state = h.state_with_mangabaka();
    assert!(
        !image(&state, &series_cover("/shadow", Some(150)))
            .await
            .is_empty()
    );
    assert_eq!(h.site.calls(), 0);

    // A refresh no longer matches the title.
    let input = h
        .lists
        .matches()
        .all("site", "refreshed")
        .unwrap()
        .into_iter()
        .find(|i| i.link == "/shadow")
        .unwrap();
    let rejected = StoredMatch {
        series_id: None,
        confidence: MatchConfidence::None,
        format: None,
        publication: None,
        year: None,
    };
    h.lists
        .matches()
        .store("site", [(&input, &rejected)])
        .unwrap();

    assert_eq!(image(&state, &series_cover("/shadow", None)).await, png());
    assert_eq!(h.site.calls(), 1);
    assert_eq!(
        h.network.urls().last().map(String::as_str),
        Some("http://site.test/covers/shadow.png")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_the_database_replaces_the_stored_mangabaka_links() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    let state = h.state_with_mangabaka();
    assert!(
        !image(&state, &series_cover("/shadow", Some(150)))
            .await
            .is_empty()
    );

    let res = build_router(state.clone())
        .oneshot(
            Request::delete("/api/metadata/mangabaka")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    assert_eq!(image(&state, &series_cover("/shadow", None)).await, png());
    assert_eq!(h.site.calls(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cover_the_series_page_learned_is_stored() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    let res = send(&h.state(), "/api/series?module=site&link=%2Fshadow").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(h.site.calls(), 1);

    let state = h.state();
    assert_eq!(image(&state, &series_cover("/shadow", None)).await, png());
    assert_eq!(h.site.calls(), 1);
    assert_eq!(h.network.urls(), ["http://site.test/covers/shadow.png"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn discover_items_point_to_their_cover() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    let state = h.state();
    let res = send(&state, "/api/lists/search?module=site&q=shadow").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let cover = body["items"][0]["cover_url"].as_str().unwrap();
    assert_eq!(cover, series_cover("/shadow", None));

    assert!(!image(&state, &format!("{cover}&w=150")).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_series_page_does_not_override_a_mangabaka_match() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    let state = h.state_with_mangabaka();
    let res = send(&state, "/api/series?module=site&link=%2Fshadow").await;
    assert_eq!(res.status(), StatusCode::OK);

    assert!(
        !image(&state, &series_cover("/shadow", Some(150)))
            .await
            .is_empty()
    );
    assert_eq!(h.network.urls(), [SHADOW_X250]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_accepted_match_replaces_a_stored_website_link() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    assert_eq!(
        image(&h.state(), &series_cover("/shadow", None)).await,
        png()
    );
    assert_eq!(h.site.calls(), 1);

    // The database is downloaded and matches the title.
    let state = h.state_with_mangabaka();
    assert!(
        !image(&state, &series_cover("/shadow", Some(150)))
            .await
            .is_empty()
    );
    assert_eq!(h.site.calls(), 1);
    assert_eq!(
        h.network.urls().last().map(String::as_str),
        Some(SHADOW_X250)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_link_is_served_when_the_website_cannot_recheck_it() {
    let h = harness(
        &[("/shadow", "http://site.test/covers/shadow.png")],
        0,
        quick(),
    );
    // Every stored link is stale at once.
    let state = h.state_with(|c| c.revalidate_after = Duration::ZERO);
    assert_eq!(image(&state, &series_cover("/shadow", None)).await, png());

    h.site.down.store(true, Ordering::SeqCst);
    let state = h.state_with(|c| c.revalidate_after = Duration::ZERO);
    assert_eq!(image(&state, &series_cover("/shadow", None)).await, png());
    assert_eq!(h.site.calls(), 2);
}
