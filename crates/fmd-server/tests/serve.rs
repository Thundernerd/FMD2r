//! The composed server: `serve(ServeConfig)` on a temp data dir whose Lua tree holds a fixture
//! module for a stub site on a local socket (docs/tickets/T37-serve-wire-catalog-covers-xpath.md,
//! "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use axum::routing::get;
use fmd_core::settings::SettingsService;
use fmd_server::{EventBus, LogBuffer, ServeConfig, serve};
use fmd_store::AppDb;
use serde_json::{Value, json};
use tempfile::TempDir;

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// The series page of the stub site.
const SERIES_PAGE: &str = r#"<html><body>
<h1>The Stub Saga</h1>
<img class="cover" src="/covers/saga.png">
<ul class="chapters">
  <li><a href="/saga/2">Chapter 2</a></li>
  <li><a href="/saga/1">Chapter 1</a></li>
</ul>
</body></html>"#;

/// A module for the stub site at `{root}`. `OnGetInfo` reads the series page; for a `/probe`
/// link it reports `name()` of an upper-case element instead, which tells the XPath backends
/// apart: `fpc` keeps the source's case, `native` lowercases it (crates/fmd-xpath/README.md,
/// "Known differences").
const MODULE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'stub'; m.Name = 'Stub'; m.RootURL = '{root}'; m.Category = 'English'
  m.OnGetInfo = 'GetInfo'
end
function GetInfo()
  if URL:find('^/probe') then
    MANGAINFO.Title = CreateTXQuery('<DIV>x</DIV>').XPathString('name(//div)')
    return no_error
  end
  if not HTTP.GET(MANGAINFO.URL) then return net_problem end
  local x = CreateTXQuery(HTTP.Document)
  MANGAINFO.Title = x.XPathString('//h1')
  MANGAINFO.CoverLink = MaybeFillHost(MODULE.RootURL, x.XPathString('//img/@src'))
  x.XPathHREFAll('//ul/li/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
  return no_error
end
"#;

/// Requests the stub site received.
#[derive(Clone, Default)]
struct Site {
    page_requests: Arc<Mutex<Vec<HeaderMap>>>,
    cover_requests: Arc<Mutex<Vec<HeaderMap>>>,
}

async fn cover(State(site): State<Site>, headers: HeaderMap) -> impl IntoResponse {
    site.cover_requests.lock().unwrap().push(headers);
    ([(header::CONTENT_TYPE, "image/png")], PNG)
}

async fn series_page(State(site): State<Site>, headers: HeaderMap) -> impl IntoResponse {
    site.page_requests.lock().unwrap().push(headers);
    ([(header::CONTENT_TYPE, "text/html")], SERIES_PAGE)
}

/// Starts the stub site; returns its root URL (`http://127.0.0.1:<port>`).
async fn start_site(site: Site) -> String {
    let app = axum::Router::new()
        .route("/saga", get(series_page))
        .route("/covers/{name}", get(cover))
        .with_state(site);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

struct Server {
    _dir: TempDir,
    site: Site,
    root: String,
    base: String,
    client: reqwest::Client,
}

impl Server {
    /// Starts the stub site, then `serve` on a data dir whose Lua tree holds the module for it.
    async fn start() -> Server {
        Server::start_with(json!({})).await
    }

    /// [`Server::start`] with `settings` (a merge patch) stored in `app.db` beforehand.
    async fn start_with(settings: Value) -> Server {
        let site = Site::default();
        let root = start_site(site.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(data_dir.join("lua/modules")).unwrap();
        std::fs::write(
            data_dir.join("lua/modules/Stub.lua"),
            MODULE.replace("{root}", &root),
        )
        .unwrap();
        let db = AppDb::open(data_dir.join("app.db")).unwrap();
        SettingsService::load(db).unwrap().update(settings).unwrap();
        let bind = free_port();
        tokio::spawn(serve(ServeConfig {
            bind,
            data_dir,
            auth: None,
            flaresolverr_url: None,
            logs: LogBuffer::new(100, EventBus::new()),
            // Tests never reach the network: no module sync with GitHub.
            module_updates: false,
        }));
        let server = Server {
            _dir: dir,
            site,
            root,
            base: format!("http://{bind}"),
            client: reqwest::Client::new(),
        };
        server.wait_until_listening().await;
        server
    }

    async fn wait_until_listening(&self) {
        for _ in 0..200 {
            if self
                .client
                .get(self.url("/api/health"))
                .send()
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the server did not start");
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.client.get(self.url(path)).send().await.unwrap()
    }

    async fn get_json(&self, path: &str) -> Value {
        let res = self.get(path).await;
        assert_eq!(res.status(), 200, "GET {path}");
        json_of(res).await
    }

    async fn send_json(&self, method: reqwest::Method, path: &str, body: Value) -> Value {
        let res = self
            .client
            .request(method.clone(), self.url(path))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{method} {path}");
        json_of(res).await
    }
}

async fn json_of(res: reqwest::Response) -> Value {
    serde_json::from_slice(&res.bytes().await.unwrap()).unwrap()
}

/// A port nothing listens on right now.
fn free_port() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_module_list_has_the_fixture_module() {
    let server = Server::start().await;

    let modules = server.get_json("/api/modules").await;

    let modules = modules.as_array().unwrap();
    assert_eq!(modules.len(), 1, "{modules:?}");
    assert_eq!(modules[0]["id"], "stub");
    assert_eq!(modules[0]["name"], "Stub");
    assert_eq!(modules[0]["category"], "English");
    assert_eq!(modules[0]["capabilities"]["info"], true);
    assert_eq!(modules[0]["capabilities"]["download"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pasted_url_resolves_and_its_series_shows_the_module_info() {
    let server = Server::start().await;

    let url = format!("{}/saga", server.root);
    let series = server
        .send_json(reqwest::Method::POST, "/api/resolve", json!({ "url": url }))
        .await;
    assert_eq!(series, json!({ "module_id": "stub", "link": "/saga" }));

    let info = server
        .get_json("/api/series?module=stub&link=%2Fsaga")
        .await;
    assert_eq!(info["title"], "The Stub Saga");
    let chapters: Vec<(&str, &str)> = info["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["name"].as_str().unwrap(), c["link"].as_str().unwrap()))
        .collect();
    assert_eq!(
        chapters,
        [("Chapter 2", "/saga/2"), ("Chapter 1", "/saga/1")]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_series_and_its_cover_are_fetched_with_the_module_http_settings() {
    let server = Server::start().await;
    // The module's HTTP settings (`PrepareHTTP`, baseunits/WebsiteModules.pas:362-366).
    let patch = json!({ "enabled": true, "http": { "user_agent": "StubAgent/1.0" } });
    server
        .send_json(reqwest::Method::PATCH, "/api/modules/stub/settings", patch)
        .await;

    let info = server
        .get_json("/api/series?module=stub&link=%2Fsaga")
        .await;
    let pages = server.site.page_requests.lock().unwrap().clone();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0][header::USER_AGENT], "StubAgent/1.0");
    let cover_url = info["cover_url"].as_str().unwrap();
    let res = server.get(cover_url).await;

    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(res.bytes().await.unwrap(), PNG);
    let seen = server.site.cover_requests.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0][header::USER_AGENT], "StubAgent/1.0");
    assert_eq!(
        seen[0][header::REFERER],
        format!("{}/", server.root).as_str()
    );
}

impl Server {
    /// The element name the fixture's `/probe` link reports: `DIV` from the `fpc` backend, `div`
    /// from `native`. `n` makes each probe a series of its own, so none comes from the cache.
    async fn probe_xpath(&self, n: u32) -> String {
        let info = self
            .get_json(&format!("/api/series?module=stub&link=%2Fprobe%2F{n}"))
            .await;
        info["title"].as_str().unwrap().to_owned()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn module_xpath_runs_on_the_native_backend_by_default() {
    let server = Server::start().await;

    assert_eq!(server.probe_xpath(1).await, "div");
}

#[cfg(feature = "xpath-fpc")]
#[tokio::test(flavor = "multi_thread")]
async fn the_stored_xpath_backend_applies_at_startup() {
    let server = Server::start_with(json!({ "xpath": { "backend": "fpc" } })).await;

    assert_eq!(server.probe_xpath(1).await, "DIV");
}

#[cfg(feature = "xpath-fpc")]
#[tokio::test(flavor = "multi_thread")]
async fn changing_the_xpath_backend_applies_without_a_restart() {
    let server = Server::start().await;
    assert_eq!(server.probe_xpath(0).await, "div");
    let mut probes = 1..;
    // The workers switch once the server sees the change, soon after the PATCH answers.
    let mut switch_to = async |backend, name: &str| {
        server
            .send_json(
                reqwest::Method::PATCH,
                "/api/settings",
                json!({ "xpath": { "backend": backend } }),
            )
            .await;
        for _ in 0..100 {
            if server.probe_xpath(probes.next().unwrap()).await == name {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("module XPath did not switch to {backend}");
    };

    switch_to("fpc", "DIV").await;
    switch_to("native", "div").await;
}
