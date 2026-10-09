//! `POST /api/resolve` and `GET /api/series` driven through `build_router` with `oneshot`, over a
//! real `WorkerPool` running fixture Lua modules whose HTTP goes to a stub transport
//! (docs/tickets/T24-series-page-add-by-url.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fmd_core::info::{InfoError, InfoOptions, MangaInfo};
use fmd_core::modules::ModuleInfo;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{ModuleRegistry, PoolConfig, WorkerPool};
use fmd_server::{AppState, ModuleCatalog, ModulesReport, build_router};
use fmd_store::AppDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// The ticket's fixture module: fetches the series page and reads its info from the body, one
/// `key=value` per line, with `chapter=<link>|<name>` lines in module order.
const T: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 't'
  m.Name = 'T'
  m.RootURL = 'https://example.com'
  m.OnGetInfo = 'GetInfo'
end

function GetInfo()
  if not HTTP.GET(MANGAINFO.URL) then return net_problem end
  local body = HTTP.Document.ToString()
  if body == 'missing' then return information_not_found end
  for line in body:gmatch('[^\n]+') do
    local key, value = line:match('^(%w+)=(.*)$')
    if key == 'chapter' then
      local link, name = value:match('^(.-)|(.*)$')
      MANGAINFO.ChapterLinks.Add(link)
      MANGAINFO.ChapterNames.Add(name)
    elseif key then
      MANGAINFO[key] = value
    end
  end
  return no_error
end
"#;

/// Answers each URL from a fixed map; any other URL is a transport error.
#[derive(Default)]
struct Site {
    pages: Mutex<HashMap<String, String>>,
    hits: Mutex<Vec<String>>,
}

impl Site {
    fn page(&self, url: &str, body: &str) {
        self.pages.lock().unwrap().insert(url.into(), body.into());
    }

    fn hits(&self, url: &str) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|u| *u == url)
            .count()
    }
}

impl Transport for Site {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.hits.lock().unwrap().push(request.url.clone());
        let page = self.pages.lock().unwrap().get(&request.url).cloned();
        Box::pin(async move {
            let body = page.ok_or_else(|| TransportError("connection refused".into()))?;
            Ok(WireResponse {
                status: 200,
                reason: "OK".into(),
                headers: vec![("Content-Type".into(), "text/plain".into())],
                body: body.into_bytes(),
            })
        })
    }
}

/// The loaded fixture modules and the pool running their callbacks.
struct Modules {
    registry: ModuleRegistry,
    pool: Arc<WorkerPool>,
}

impl ModuleCatalog for Modules {
    fn report(&self) -> ModulesReport {
        ModulesReport::default()
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        let mut modules: Vec<_> = self
            .registry
            .modules()
            .iter()
            .map(|m| ModuleInfo::from(&m.def()))
            .collect();
        modules.sort_by(|a, b| a.id.cmp(&b.id));
        modules
    }

    fn get_info(
        &self,
        id: &str,
        link: &str,
        options: InfoOptions,
    ) -> BoxFuture<'static, Result<MangaInfo, InfoError>> {
        let module = self.registry.get(id).cloned();
        let pool = self.pool.clone();
        let link = link.to_owned();
        Box::pin(async move {
            let module = module.ok_or(InfoError::UnknownModule)?;
            fmd_core::info::get_info(&pool, &module, &link, options).await
        })
    }
}

struct Harness {
    _dir: TempDir,
    state: AppState,
    db: AppDb,
    site: Arc<Site>,
}

fn harness(modules: &[(&str, &str)]) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(dir.path().join("app.db")).unwrap();
    let lua = dir.path().join("lua");
    std::fs::create_dir_all(lua.join("modules")).unwrap();
    for (name, source) in modules {
        std::fs::write(lua.join("modules").join(name), source).unwrap();
    }
    let report = ModuleRegistry::load_dir(&lua);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let site = Arc::new(Site::default());
    let mut config = PoolConfig::new(HttpClient::with_transport(site.clone()).unwrap());
    config.threads = 1;
    config.lua_dir = lua;
    let pool = Arc::new(WorkerPool::new(config).unwrap());
    let state = AppState::new(db.clone()).unwrap().with_modules(Modules {
        registry: report.registry,
        pool,
    });
    Harness {
        _dir: dir,
        state,
        db,
        site,
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Response {
    build_router(state.clone()).oneshot(req).await.unwrap()
}

async fn body_json(res: Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn resolve(state: &AppState, url: &str) -> Response {
    let req = Request::post("/api/resolve")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "url": url }).to_string()))
        .unwrap();
    send(state, req).await
}

#[tokio::test]
async fn resolve_finds_the_module_by_host_and_keeps_the_path_as_link() {
    let h = harness(&[("T.lua", T)]);

    let res = resolve(&h.state, "https://example.com/manga/1").await;

    assert_eq!(res.status(), StatusCode::OK);
    // edURLButtonClick (mangadownloader/forms/frmMain.pas:6578-6606): `SplitURL` gives the host
    // to `LocateModuleByHost` and the path is the link FMD2 opens and stores.
    assert_eq!(
        body_json(res).await,
        json!({ "module_id": "t", "link": "/manga/1" })
    );
}

/// A module whose `RootURL` has a `www.` host.
const W: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'w'
  m.Name = 'W'
  m.RootURL = 'https://www.Wsite.net'
end
"#;

async fn resolved(state: &AppState, url: &str) -> Option<(String, String)> {
    let res = resolve(state, url).await;
    match res.status() {
        StatusCode::OK => {
            let body = body_json(res).await;
            Some((
                body["module_id"].as_str().unwrap().into(),
                body["link"].as_str().unwrap().into(),
            ))
        }
        StatusCode::NOT_FOUND => None,
        status => panic!("{url}: {status}"),
    }
}

fn found(module: &str, link: &str) -> Option<(String, String)> {
    Some((module.into(), link.into()))
}

#[tokio::test]
async fn resolve_matches_hosts_as_locate_module_by_host_does() {
    let h = harness(&[("T.lua", T), ("W.lua", W)]);
    let r = |url: &'static str| resolved(&h.state, url);

    // LocateModuleByHost (baseunits/WebsiteModules.pas:500-530) first looks for the whole
    // lowercased `scheme://host` in each RootURL (lowercased by the loader) ...
    assert_eq!(
        r("https://EXAMPLE.com/Manga/1").await,
        found("t", "/Manga/1")
    );
    // ... then for the host without scheme or port ...
    assert_eq!(
        r("http://example.com/manga/1").await,
        found("t", "/manga/1")
    );
    assert_eq!(
        r("https://example.com:8443/manga/1?x=1").await,
        found("t", "/manga/1?x=1")
    );
    assert_eq!(r("example.com/manga/1").await, found("t", "/manga/1"));
    assert_eq!(r("https://wsite.net/s/2").await, found("w", "/s/2"));
    // ... then without its first four characters when it starts with `www.` (or holds a `w`).
    assert_eq!(
        r("https://www.example.com/manga/1").await,
        found("t", "/manga/1")
    );
    assert_eq!(
        r("http://ww2.example.com/manga/1").await,
        found("t", "/manga/1")
    );
    // Other subdomains of a module's host match nothing.
    assert_eq!(r("https://m.example.com/manga/1").await, None);
    // edURLButtonClick (mangadownloader/forms/frmMain.pas:6589-6600) needs a host and a path.
    assert_eq!(r("https://example.com").await, None);
    assert_eq!(r("https://example.com/").await, None);
    assert_eq!(r("/manga/1").await, None);
}

#[tokio::test]
async fn resolve_reports_an_unknown_host_as_a_404_with_a_message() {
    let h = harness(&[("T.lua", T)]);

    let res = resolve(&h.state, "https://nowhere.org/manga/1").await;

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let detail = body_json(res).await["detail"].as_str().unwrap().to_owned();
    assert!(detail.contains("nowhere.org"), "{detail}");
}

#[tokio::test]
async fn resolve_picks_the_last_matching_module_by_id() {
    // `PosModule` walks the ID-sorted list from the end (baseunits/WebsiteModules.pas:503-510).
    let mirror = T
        .replace("'t'", "'z'")
        .replace("example.com'", "example.com.mirror'");
    let earlier = T.replace("'t'", "'a'");
    let h = harness(&[("T.lua", T), ("Z.lua", &mirror), ("A.lua", &earlier)]);

    assert_eq!(
        resolved(&h.state, "https://example.com/manga/1").await,
        found("z", "/manga/1")
    );
}

const SERIES_PAGE: &str = "Title=One Piece\nAuthors=Oda\nStatus=1\nGenres=Action, Adventure\nSummary=Pirates.\nCoverLink=https://example.com/cover/1.jpg\nchapter=/manga/1/c3|Chapter 3\nchapter=/manga/1/c1|Chapter 1\nchapter=/manga/1/c2|Chapter 2\n";

async fn series(state: &AppState, module: &str, link: &str) -> Response {
    let query = url_query(&[("module", module), ("link", link)]);
    send(
        state,
        Request::get(format!("/api/series?{query}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

fn url_query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            let v: String = v
                .bytes()
                .map(|b| match b {
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect();
            format!("{k}={v}")
        })
        .collect::<Vec<_>>()
        .join("&")
}

#[tokio::test]
async fn series_shows_the_modules_info_and_chapters_with_their_downloaded_state() {
    let h = harness(&[("T.lua", T)]);
    h.site.page("https://example.com/manga/1", SERIES_PAGE);
    // FMD2 keys downloaded chapters by module and series link and finds chapter links
    // case-insensitively (baseunits/uDownloadsManager.pas:1747-1768).
    h.db.downloaded_chapters()
        .mark("t", "/manga/1", &["/manga/1/C1"])
        .unwrap();

    let res = series(&h.state, "t", "/manga/1").await;

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["module_id"], "t");
    assert_eq!(body["link"], "/manga/1");
    assert_eq!(body["title"], "One Piece");
    assert_eq!(body["authors"], "Oda");
    assert_eq!(body["status"], "ongoing");
    assert_eq!(body["genres"], json!(["Action", "Adventure"]));
    assert_eq!(body["summary"], "Pirates.");
    assert_eq!(body["in_library"], false);
    assert_eq!(
        body["cover_url"],
        "/api/covers?module=t&url=https%3A%2F%2Fexample.com%2Fcover%2F1.jpg"
    );
    // In module order, as `ChapterLinks` holds them.
    assert_eq!(
        body["chapters"],
        json!([
            { "name": "Chapter 3", "link": "/manga/1/c3", "downloaded": false },
            { "name": "Chapter 1", "link": "/manga/1/c1", "downloaded": true },
            { "name": "Chapter 2", "link": "/manga/1/c2", "downloaded": false },
        ])
    );
}

#[tokio::test]
async fn series_info_is_cleaned_up_as_get_info_from_url_does() {
    let h = harness(&[("T.lua", T)]);
    h.site.page(
        "https://example.com/manga/2",
        concat!(
            "Link=/manga/two\n",
            "AltTitles=-\n",
            "Authors=  Oda,  \n",
            "Artists=<a>\n",
            "Summary=:\n",
            "CoverLink=https://cdn.example.com//covers//2.jpg\n",
            "chapter=/manga/two/c1|Old &amp; first\n",
            "chapter=/manga/two/c2|  Two\n",
            "chapter=/manga/two/C1|New &amp; first\n",
            "chapter=/manga/two/c3|\n",
            "chapter=https://example.com/manga/two/c4|Four\n",
        ),
    );
    h.db.downloaded_chapters()
        .mark("t", "/manga/two", &["/manga/two/c2"])
        .unwrap();

    let body = body_json(series(&h.state, "t", "/manga/2").await).await;

    // GetInfoFromURL (baseunits/uData.pas:111-205): an empty title becomes `N/A`, placeholder
    // values (`-`, `:`, markup) are dropped, trailing commas trimmed, doubled slashes in the cover
    // link collapsed.
    assert_eq!(body["title"], "N/A");
    assert_eq!(body["alt_titles"], "");
    assert_eq!(body["authors"], "Oda");
    assert_eq!(body["artists"], "");
    assert_eq!(body["summary"], "");
    assert_eq!(body["status"], "unknown");
    assert_eq!(body["genres"], json!([]));
    assert_eq!(
        body["cover_url"],
        "/api/covers?module=t&url=https%3A%2F%2Fcdn.example.com%2Fcovers%2F2.jpg"
    );
    // The module's own link wins, and keys the downloaded chapters.
    assert_eq!(body["link"], "/manga/two");
    // Duplicate links (ignoring case) keep the last one, hosts are removed from links, names are
    // entity-decoded and trimmed, and a missing name is empty.
    assert_eq!(
        body["chapters"],
        json!([
            { "name": "Two", "link": "/manga/two/c2", "downloaded": true },
            { "name": "New & first", "link": "/manga/two/C1", "downloaded": false },
            { "name": "", "link": "/manga/two/c3", "downloaded": false },
            { "name": "Four", "link": "/manga/two/c4", "downloaded": false },
        ])
    );
}

#[tokio::test]
async fn series_maps_module_failures_to_http_errors() {
    let h = harness(&[("T.lua", T)]);
    h.site.page("https://example.com/manga/gone", "missing");

    // `information_not_found`: nothing there.
    let res = series(&h.state, "t", "/manga/gone").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    // `net_problem`: the site failed to answer (no page is scripted for this one).
    let res = series(&h.state, "t", "/manga/down").await;
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    // No such module.
    let res = series(&h.state, "nope", "/manga/1").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    // No link.
    let res = series(&h.state, "t", "").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn series_info_is_cached_briefly_but_library_and_downloaded_state_are_live() {
    let h = harness(&[("T.lua", T)]);
    h.site.page("https://example.com/manga/1", SERIES_PAGE);

    let first = body_json(series(&h.state, "t", "/manga/1").await).await;
    h.db.downloaded_chapters()
        .mark("t", "/manga/1", &["/manga/1/c2"])
        .unwrap();
    h.db.favorites()
        .create(&fmd_store::NewFavorite {
            module_id: "t".into(),
            link: "/manga/1".into(),
            title: "One Piece".into(),
            save_to: "/downloads".into(),
            cover_url: None,
        })
        .unwrap();
    let second = body_json(series(&h.state, "t", "/manga/1").await).await;

    assert_eq!(h.site.hits("https://example.com/manga/1"), 1);
    assert_eq!(first["title"], second["title"]);
    assert_eq!(first["in_library"], false);
    assert_eq!(second["in_library"], true);
    assert_eq!(second["chapters"][2]["downloaded"], true);
}

#[tokio::test]
async fn series_chapter_names_lose_the_title_when_the_setting_says_so() {
    let h = harness(&[("T.lua", T)]);
    h.site.page(
        "https://example.com/manga/3",
        "Title=One Piece\nchapter=/c1|One Piece - Chapter 1\nchapter=/c2|ONE PIECE Extra\nchapter=/c3|One Piece\n",
    );
    let patch = Request::patch("/api/settings")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "saveto": { "remove_manga_name_from_chapter": true } }).to_string(),
        ))
        .unwrap();
    assert_eq!(send(&h.state, patch).await.status(), StatusCode::OK);

    let body = body_json(series(&h.state, "t", "/manga/3").await).await;

    // OptionRemoveMangaNameFromChapter (baseunits/uData.pas:183-200): a leading title (any case)
    // and a following `- ` go, unless the name is no longer than the title.
    let names: Vec<_> = body["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Chapter 1", "Extra", "One Piece"]);
}
