//! The composed server: `serve(ServeConfig)` on a temp data dir whose Lua tree holds a fixture
//! module for a stub site on a local socket (docs/tickets/T37-serve-wire-catalog-covers-xpath.md
//! docs/tickets/T38-serve-wire-accounts-lists-favorites.md and
//! docs/tickets/T54-apply-connection-settings.md, "Seams under test").
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use axum::routing::get;
use fmd_core::settings::SettingsService;
use fmd_server::{EventBus, LogBuffer, ServeConfig, serve};
use fmd_store::{AppDb, NewFavorite};
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
  m.AccountSupport = true
  m.OnLogin = 'Login'
  m.OnGetDirectoryPageNumber = 'GetDirectoryPageNumber'
  m.OnGetNameAndLink = 'GetNameAndLink'
end
function GetDirectoryPageNumber()
  PAGENUMBER = 1
  return no_error
end
function GetNameAndLink()
  if not HTTP.GET(MODULE.RootURL .. '/list/' .. URL) then return net_problem end
  for line in HTTP.Document.ToString():gmatch('[^\n]+') do
    local link, name = line:match('^(%S+) (.+)$')
    LINKS.Add(link)
    NAMES.Add(name)
  end
  return no_error
end
function Login()
  if MODULE.Account.Username == 'reader' and MODULE.Account.Password == 'secret' then
    MODULE.Account.Status = asValid; return true
  end
  MODULE.Account.Status = asInvalid; return false
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

/// The stub site's directory: one `link name` line per title.
const DIRECTORY: &str = "/saga The Stub Saga\n/tale A Stub Tale\n";

/// Requests the stub site received.
#[derive(Clone, Default)]
struct Site {
    /// Lists a third chapter on the series page once set.
    chapter_3: Arc<AtomicBool>,
    page_requests: Arc<Mutex<Vec<HeaderMap>>>,
    cover_requests: Arc<Mutex<Vec<HeaderMap>>>,
}

async fn cover(State(site): State<Site>, headers: HeaderMap) -> impl IntoResponse {
    site.cover_requests.lock().unwrap().push(headers);
    ([(header::CONTENT_TYPE, "image/png")], PNG)
}

async fn series_page(State(site): State<Site>, headers: HeaderMap) -> impl IntoResponse {
    site.page_requests.lock().unwrap().push(headers);
    let page = if site.chapter_3.load(Ordering::SeqCst) {
        SERIES_PAGE.replace(
            "<ul class=\"chapters\">",
            "<ul class=\"chapters\">\n  <li><a href=\"/saga/3\">Chapter 3</a></li>",
        )
    } else {
        SERIES_PAGE.to_owned()
    };
    ([(header::CONTENT_TYPE, "text/html")], page)
}

/// Starts the stub site; returns its root URL (`http://127.0.0.1:<port>`).
async fn start_site(site: Site) -> String {
    let app = axum::Router::new()
        .route("/saga", get(series_page))
        .route("/tale", get(series_page))
        .route("/series/{n}", get(series_page))
        .route("/list/0", get(|| async { DIRECTORY }))
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
        Server::start_seeded(Site::default(), settings, |_, _| {}).await
    }

    /// [`Server::start_with`] on `site`, with `seed` run on `app.db` and the Lua dir beforehand.
    async fn start_seeded(site: Site, settings: Value, seed: impl FnOnce(&AppDb, &Path)) -> Server {
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
        seed(&db, &data_dir.join("lua"));
        SettingsService::load(db)
            .unwrap()
            .update(settings.clone())
            .unwrap();
        let bind = free_port();
        // Tests never reach the network: the module updater runs only on request, and none is
        // made.
        let module_updates = settings["module_updater"]["auto_update"] == false;
        tokio::spawn(serve(ServeConfig {
            bind,
            data_dir,
            auth: None,
            flaresolverr_url: None,
            logs: LogBuffer::new(100, EventBus::new()),
            module_updates,
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

#[tokio::test(flavor = "multi_thread")]
async fn the_fixture_account_logs_in_with_its_credentials() {
    let server = Server::start().await;

    let credentials = json!({ "username": "reader", "password": "secret", "enabled": true });
    server
        .send_json(reqwest::Method::PUT, "/api/accounts/stub", credentials)
        .await;
    // `TAccountCheckThread` (mangadownloader/forms/frmAccountManager.pas:125-136).
    let account = server
        .send_json(reqwest::Method::POST, "/api/accounts/stub/login", json!({}))
        .await;

    assert_eq!(account["status"], "valid", "{account}");
}

impl Server {
    /// Polls `GET {path}` until `done` holds for its JSON body; returns that body.
    async fn wait_for(&self, path: &str, done: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..200 {
            let body = self.get_json(path).await;
            if done(&body) {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("GET {path} never got there");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn updating_the_list_makes_the_site_titles_searchable() {
    let server = Server::start().await;

    let res = server
        .client
        .post(server.url("/api/lists/stub/update"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202);

    // `TUpdateListManagerThread` (baseunits/uUpdateThread.pas) adds every title the directory
    // lists.
    let found = server
        .wait_for("/api/lists/search?module=stub&q=stub", |page| {
            page["total"] == 2
        })
        .await;
    let mut links: Vec<&str> = found["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["link"].as_str().unwrap())
        .collect();
    links.sort_unstable();
    assert_eq!(links, ["/saga", "/tale"]);
}

impl Server {
    /// Adds the stub's `/saga` to the library, with its chapters so far counted as downloaded.
    async fn add_saga_to_the_library(&self) {
        let res = self
            .client
            .post(self.url("/api/favorites"))
            .header("content-type", "application/json")
            .body(json!({ "module_id": "stub", "link": "/saga" }).to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201);
    }

    /// Runs the new-chapter check and waits for it to end.
    async fn check_favorites(&self) {
        let res = self
            .client
            .post(self.url("/api/favorites/check"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 202);
        self.wait_for("/api/jobs/favorites", |job| job["state"] == "done")
            .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_favorites_check_reports_the_new_chapter_in_the_inbox() {
    let server = Server::start_with(json!({ "favorites": { "check_at_startup": false } })).await;
    server.add_saga_to_the_library().await;
    server.site.chapter_3.store(true, Ordering::SeqCst);

    server.check_favorites().await;

    // `ShowResult` (baseunits/uFavoritesManager.pas:1047-1073) lists the new chapter.
    let inbox = server.get_json("/api/inbox").await;
    let items = inbox.as_array().unwrap();
    assert_eq!(items.len(), 1, "{inbox}");
    assert_eq!(items[0]["title"], "Found new chapter(s)");
    assert!(
        items[0]["body"].as_str().unwrap().contains("The Stub Saga"),
        "{inbox}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_auto_download_a_favorites_check_queues_the_new_chapter() {
    let server = Server::start_with(json!({
        "favorites": { "check_at_startup": false, "auto_download": true }
    }))
    .await;
    server.add_saga_to_the_library().await;
    server.site.chapter_3.store(true, Ordering::SeqCst);

    server.check_favorites().await;

    // `ShowResult` queues the new chapters instead (baseunits/uFavoritesManager.pas:1047-1073).
    let tasks = server.get_json("/api/tasks").await;
    let items = tasks["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{tasks}");
    assert_eq!(items[0]["module_id"], "stub");
    assert_eq!(items[0]["link"], "/saga");
    assert_eq!(items[0]["chapter_count"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_jobs_are_the_module_updater_the_favorites_check_and_the_list_updates() {
    let server = Server::start_with(json!({ "module_updater": { "auto_update": false } })).await;
    let res = server
        .client
        .post(server.url("/api/lists/stub/update"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 202);

    let jobs = server
        .wait_for("/api/jobs", |jobs| {
            jobs.as_array()
                .unwrap()
                .iter()
                .any(|job| job["id"] == "lists" && job["state"] == "done")
        })
        .await;

    let mut ids: Vec<&str> = jobs
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["favorites", "lists", "modules"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_favorites_are_checked_at_startup() {
    let site = Site::default();
    site.chapter_3.store(true, Ordering::SeqCst);
    // The library holds `/saga` with its first two chapters downloaded.
    let seed = |db: &AppDb, _: &Path| {
        db.favorites()
            .create(&NewFavorite {
                module_id: "stub".into(),
                link: "/saga".into(),
                title: "The Stub Saga".into(),
                save_to: String::new(),
                cover_url: None,
            })
            .unwrap();
        db.downloaded_chapters()
            .mark("stub", "/saga", &["/saga/1", "/saga/2"])
            .unwrap();
    };

    // `check_at_startup` is on by default (`tmStartupTimer`, mangadownloader/forms/frmMain.pas:2078-2082).
    let server = Server::start_seeded(site, json!({}), seed).await;

    let inbox = server
        .wait_for("/api/inbox", |inbox| !inbox.as_array().unwrap().is_empty())
        .await;
    assert_eq!(inbox[0]["title"], "Found new chapter(s)", "{inbox}");
}

/// The commit the fixture Lua tree was snapshotted at.
const SNAPSHOT_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[tokio::test(flavor = "multi_thread")]
async fn about_shows_the_followed_ref_and_the_snapshot_sha_before_any_sync() {
    let seed = |_: &AppDb, lua: &Path| {
        std::fs::write(lua.join("UPSTREAM_REF"), format!("{SNAPSHOT_SHA}\n")).unwrap();
    };
    let server = Server::start_seeded(
        Site::default(),
        json!({ "module_updater": { "auto_update": false, "repo_ref": "stable" } }),
        seed,
    )
    .await;

    let about = server.get_json("/api/about").await;

    assert_eq!(about["upstream_ref"], "stable", "{about}");
    assert_eq!(about["upstream_sha"], SNAPSHOT_SHA, "{about}");
}

#[tokio::test(flavor = "multi_thread")]
async fn about_shows_the_synced_commit_once_the_tree_was_synced() {
    let synced = "89abcdef0123456789abcdef0123456789abcdef";
    // A previous run's updater synced the seeded tree to `synced`.
    let seed = |db: &AppDb, lua: &Path| {
        std::fs::write(lua.join("UPSTREAM_REF"), SNAPSHOT_SHA).unwrap();
        db.settings()
            .set("module_updater.repo", &json!({ "last_commit_sha": synced }))
            .unwrap();
    };
    let server = Server::start_seeded(
        Site::default(),
        json!({ "module_updater": { "auto_update": false } }),
        seed,
    )
    .await;

    let about = server.get_json("/api/about").await;

    assert_eq!(about["upstream_ref"], "master", "{about}");
    assert_eq!(about["upstream_sha"], synced, "{about}");
}

#[tokio::test(flavor = "multi_thread")]
async fn about_lists_the_module_that_failed_to_load() {
    let seed = |_: &AppDb, lua: &Path| {
        std::fs::write(
            lua.join("modules/Broken.lua"),
            "function Init() error('no site today') end",
        )
        .unwrap();
    };
    let server = Server::start_seeded(
        Site::default(),
        json!({ "module_updater": { "auto_update": false } }),
        seed,
    )
    .await;

    let about = server.get_json("/api/about").await;

    assert_eq!(about["module_count"], 1, "{about}");
    let failures = about["load_failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1, "{about}");
    assert_eq!(failures[0]["module"], "modules/Broken.lua");
    let error = failures[0]["error"].as_str().unwrap();
    assert!(error.contains("no site today"), "{error}");
}

/// A forward HTTP proxy on a local socket: records the absolute URI of each request it gets and
/// passes the request on to the site.
#[derive(Clone, Default)]
struct StubProxy {
    seen: Arc<Mutex<Vec<String>>>,
}

impl StubProxy {
    /// Starts the proxy; returns its port.
    async fn start(&self) -> u16 {
        let proxy = self.clone();
        let app = axum::Router::new().fallback(
            move |method: reqwest::Method, uri: axum::http::Uri, headers: HeaderMap| {
                let proxy = proxy.clone();
                async move {
                    proxy.seen.lock().unwrap().push(uri.to_string());
                    let client = reqwest::Client::builder().no_proxy().build().unwrap();
                    let mut request = client.request(method, uri.to_string());
                    for (name, value) in &headers {
                        if name != header::HOST {
                            request = request.header(name, value);
                        }
                    }
                    let res = request.send().await.unwrap();
                    let content_type = res.headers()[header::CONTENT_TYPE].clone();
                    (
                        [(header::CONTENT_TYPE, content_type)],
                        res.bytes().await.unwrap(),
                    )
                }
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        port
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

/// The `connections.proxy` setting for an HTTP proxy on `port` of this host.
fn proxy_on(port: u16) -> Value {
    json!({ "connections": { "proxy": {
        "enabled": true, "type": "http", "host": "127.0.0.1", "port": port
    } } })
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_proxy_on_at_startup_module_requests_go_through_it() {
    let proxy = StubProxy::default();
    let port = proxy.start().await;
    let server = Server::start_with(proxy_on(port)).await;

    let info = server
        .get_json("/api/series?module=stub&link=%2Fsaga")
        .await;

    assert_eq!(info["title"], "The Stub Saga");
    // `SetDefaultProxyAndApply` from `ApplyOptions` at startup
    // (mangadownloader/forms/frmMain.pas:6287-6295).
    assert_eq!(proxy.seen(), [format!("{}/saga", server.root)]);
}

impl Server {
    /// PATCHes `settings`, then fetches fresh series (`/series/<n>`, never cached) until
    /// `applied` holds after one: the server applies the change soon after the PATCH answers.
    async fn patch_settings_until(&self, settings: Value, mut applied: impl FnMut() -> bool) {
        static SERIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        self.send_json(reqwest::Method::PATCH, "/api/settings", settings)
            .await;
        for _ in 0..100 {
            let n = SERIES.fetch_add(1, Ordering::SeqCst);
            self.get_json(&format!("/api/series?module=stub&link=%2Fseries%2F{n}"))
                .await;
            if applied() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the settings change never applied");
    }
}

impl StubProxy {
    /// A check for [`Server::patch_settings_until`]: whether the request made since the previous
    /// check went through this proxy (`through`) or around it.
    fn last_request(&self, through: bool) -> impl FnMut() -> bool {
        let proxy = self.clone();
        let mut seen = proxy.seen().len();
        move || {
            let now = proxy.seen().len();
            let proxied = now > seen;
            seen = now;
            proxied == through
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_the_proxy_on_and_off_applies_without_a_restart() {
    let proxy = StubProxy::default();
    let port = proxy.start().await;
    let server = Server::start().await;
    server
        .get_json("/api/series?module=stub&link=%2Fsaga")
        .await;
    assert_eq!(proxy.seen(), Vec::<String>::new());

    // `ApplyOptions` on save calls `SetDefaultProxyAndApply`
    // (mangadownloader/forms/frmMain.pas:6287-6295).
    server
        .patch_settings_until(proxy_on(port), proxy.last_request(true))
        .await;
    let off = json!({ "connections": { "proxy": { "enabled": false } } });
    server
        .patch_settings_until(off, proxy.last_request(false))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_user_agent_is_sent_by_new_sessions() {
    let server = Server::start_with(json!({ "connections": { "user_agent": "Before/1.0" } })).await;
    server
        .get_json("/api/series?module=stub&link=%2Fsaga")
        .await;
    let last_user_agent = || {
        let pages = server.site.page_requests.lock().unwrap();
        pages.last().unwrap()[header::USER_AGENT].clone()
    };
    assert_eq!(last_user_agent(), "Before/1.0");

    // `ApplyOptions` sets `DefaultUserAgent` for sessions created afterwards
    // (mangadownloader/forms/frmMain.pas:6279-6285); each module call gets a new one.
    let patch = json!({ "connections": { "user_agent": "After/2.0" } });
    server
        .patch_settings_until(patch, || last_user_agent() == "After/2.0")
        .await;
}
