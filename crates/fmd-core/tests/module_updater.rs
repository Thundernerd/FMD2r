//! The module updater against a stub GitHub (API and raw downloads behind injectable base URLs),
//! a temp Lua dir and a temp `app.db` (docs/tickets/T29-module-updater.md, "Seams under test").
//!
//! Expected values come from FMD2's updater: the ETag and commit/tree flow of
//! `TGitHubRepo` (baseunits/GitHubRepoV3.pas:163-272) and the SHA diff, deletes and downloads of
//! `TCheckUpdateThread` (mangadownloader/forms/frmLuaModulesUpdater.pas:588-890).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fmd_core::module_updater::{
    LiveModules, ModuleUpdater, RepoConfig, UpdateError, UpdaterConfig,
};
use fmd_core::settings::write_websitebypass_config;
use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{MemorySettingsStore, PoolConfig, WorkerPool};
use fmd_store::{AppDb, EventQuery, EventSeverity};
use serde_json::json;

const API: &str = "http://api.github.test/";
const RAW: &str = "http://raw.github.test/";

/// What the stub repository holds: its last commit, the ETag of the commits answer, and the
/// files under `lua/` by path, with their blob SHAs.
#[derive(Default)]
struct Remote {
    commit: String,
    etag: String,
    files: BTreeMap<String, (String, Vec<u8>)>,
    /// Files whose download fails.
    unavailable: Vec<String>,
    /// When set, the API answers 403 with no requests left until this Unix time.
    rate_limited_until: Option<i64>,
}

/// A stub of api.github.com and raw.githubusercontent.com for `owner/name`, path `lua`.
#[derive(Default)]
struct StubGitHub {
    remote: Mutex<Remote>,
    requests: Mutex<Vec<WireRequest>>,
}

impl StubGitHub {
    fn publish(&self, commit: &str, etag: &str, files: &[(&str, &str, &str)]) {
        let mut remote = self.remote.lock().unwrap();
        remote.commit = commit.into();
        remote.etag = etag.into();
        remote.files = files
            .iter()
            .map(|(path, sha, body)| (path.to_string(), (sha.to_string(), body.as_bytes().into())))
            .collect();
    }

    fn set_unavailable(&self, paths: &[&str]) {
        self.remote.lock().unwrap().unavailable = paths.iter().map(|p| p.to_string()).collect();
    }

    fn requests(&self) -> Vec<WireRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn clear_requests(&self) {
        self.requests.lock().unwrap().clear();
    }

    fn downloads(&self) -> Vec<String> {
        self.requests()
            .iter()
            .filter_map(|r| r.url.strip_prefix(RAW).map(str::to_owned))
            .collect()
    }

    fn answer(&self, request: &WireRequest) -> WireResponse {
        let remote = self.remote.lock().unwrap();
        let url = request.url.as_str();
        if let Some(reset) = remote.rate_limited_until.filter(|_| url.starts_with(API)) {
            let reset = reset.to_string();
            let headers = [
                ("X-RateLimit-Remaining", "0"),
                ("X-RateLimit-Reset", reset.as_str()),
            ];
            return response(403, &headers, b"API rate limit exceeded".to_vec());
        }
        if let Some(query) = url.strip_prefix(&format!("{API}repos/owner/name/commits?")) {
            assert_eq!(query, "sha=master&per_page=1&path=lua");
            let if_none_match = request
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("If-None-Match"))
                .map(|(_, v)| v.as_str());
            if if_none_match == Some(remote.etag.as_str()) {
                return response(304, &[], Vec::new());
            }
            let body = json!([{ "sha": remote.commit }]).to_string();
            return response(200, &[("ETag", &remote.etag)], body.into_bytes());
        }
        let tree = format!(
            "{API}repos/owner/name/git/trees/{}:lua?recursive=1",
            remote.commit
        );
        if url == tree {
            let mut entries = vec![json!({ "path": "modules", "type": "tree", "sha": "t0" })];
            for (path, (sha, _)) in &remote.files {
                entries.push(json!({ "path": path, "type": "blob", "sha": sha }));
            }
            let body = json!({ "sha": "t1", "tree": entries, "truncated": false }).to_string();
            return response(200, &[], body.into_bytes());
        }
        let prefix = format!("{RAW}owner/name/{}/lua/", remote.commit);
        if let Some((_, body)) = url
            .strip_prefix(&prefix)
            .filter(|path| !remote.unavailable.iter().any(|u| u == path))
            .and_then(|path| remote.files.get(path))
        {
            return response(200, &[], body.clone());
        }
        response(404, &[], b"Not Found".to_vec())
    }
}

impl Transport for StubGitHub {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let answer = self.answer(&request);
        self.requests.lock().unwrap().push(request);
        Box::pin(async move { Ok(answer) })
    }
}

fn response(status: u16, headers: &[(&str, &str)], body: Vec<u8>) -> WireResponse {
    WireResponse {
        status,
        reason: String::new(),
        headers: headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
        body,
    }
}

/// A module file declaring module `id` whose `GetInfo` sets the title to `title`.
fn module(id: &str, title: &str) -> String {
    format!(
        "function Init() local m = NewWebsiteModule(); m.ID='{id}'; m.Name='{id}'; \
         m.RootURL='https://{id}'; m.OnGetInfo='GetInfo' end\n\
         function GetInfo() MANGAINFO.Title = '{title}'; return no_error end\n"
    )
}

/// A temp Lua dir and `app.db`, a stub GitHub and an updater syncing between them.
struct Fixture {
    dir: tempfile::TempDir,
    db: AppDb,
    github: Arc<StubGitHub>,
    modules: Arc<LiveModules>,
    updater: ModuleUpdater,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture::with(|_| {})
    }

    fn with(configure: impl FnOnce(&mut UpdaterConfig)) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(dir.path().join("app.db")).unwrap();
        let lua_dir = dir.path().join("lua");
        let github = Arc::new(StubGitHub::default());
        let http = HttpClient::with_transport(github.clone()).unwrap();
        let modules = Arc::new(LiveModules::load(
            &lua_dir,
            Arc::new(MemorySettingsStore::new()),
        ));
        let mut config = UpdaterConfig {
            repo: RepoConfig {
                api_url: API.into(),
                download_url: RAW.into(),
                owner: "owner".into(),
                name: "name".into(),
                git_ref: "master".into(),
                path: "lua".into(),
            },
            ..UpdaterConfig::new(lua_dir)
        };
        configure(&mut config);
        let updater = ModuleUpdater::new(config, db.clone(), http, modules.clone());
        Fixture {
            dir,
            db,
            github,
            modules,
            updater,
        }
    }

    fn lua_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("lua")
    }

    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.lua_dir().join(path)).ok()
    }

    fn rows(&self) -> Vec<(String, String)> {
        let files = self.db.module_files().list().unwrap();
        files.into_iter().map(|f| (f.path, f.sha)).collect()
    }
}

#[test]
fn first_run_downloads_every_file_of_the_tree() {
    let f = Fixture::new();
    let a = module("a", "A1");
    let b = module("b", "B1");
    f.github.publish(
        "c1",
        "\"e1\"",
        &[
            ("modules/A.lua", "sa1", &a),
            ("modules/B.lua", "sb1", &b),
            ("utils/helper.lua", "sh1", "return {}"),
        ],
    );

    f.updater.sync().unwrap();

    assert_eq!(f.read("modules/A.lua").as_deref(), Some(a.as_str()));
    assert_eq!(f.read("modules/B.lua").as_deref(), Some(b.as_str()));
    assert_eq!(f.read("utils/helper.lua").as_deref(), Some("return {}"));
    assert_eq!(
        f.rows(),
        [
            ("modules/A.lua".to_owned(), "sa1".to_owned()),
            ("modules/B.lua".to_owned(), "sb1".to_owned()),
            ("utils/helper.lua".to_owned(), "sh1".to_owned()),
        ]
    );
}

/// The ticket's first upstream commit: two modules and a helper they could `require`.
fn publish_first(github: &StubGitHub) {
    github.publish(
        "c1",
        "\"e1\"",
        &[
            ("modules/A.lua", "sa1", &module("a", "A1")),
            ("modules/B.lua", "sb1", &module("b", "B1")),
            ("utils/helper.lua", "sh1", "return {}"),
        ],
    );
}

#[test]
fn an_unchanged_upstream_answers_304_to_the_stored_etag_and_nothing_downloads() {
    let f = Fixture::new();
    publish_first(&f.github);
    f.updater.sync().unwrap();
    f.github.clear_requests();

    let report = f.updater.sync().unwrap();

    let requests = f.github.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let if_none_match: Vec<_> = requests[0]
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("If-None-Match"))
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(if_none_match, ["\"e1\""]);
    assert!(f.github.downloads().is_empty());
    assert!(report.downloaded.is_empty());
    assert_eq!(f.rows().len(), 3);
}

#[test]
fn a_new_tree_downloads_changed_files_and_deletes_removed_ones() {
    let f = Fixture::new();
    publish_first(&f.github);
    f.updater.sync().unwrap();
    f.github.clear_requests();
    let a2 = module("a", "A2");
    f.github.publish(
        "c2",
        "\"e2\"",
        &[
            ("modules/A.lua", "sa2", &a2),
            ("modules/B.lua", "sb1", &module("b", "B1")),
        ],
    );

    let report = f.updater.sync().unwrap();

    assert_eq!(f.github.downloads(), ["owner/name/c2/lua/modules/A.lua"]);
    assert_eq!(report.downloaded, ["modules/A.lua"]);
    assert_eq!(report.deleted, ["utils/helper.lua"]);
    assert_eq!(f.read("modules/A.lua").as_deref(), Some(a2.as_str()));
    assert_eq!(f.read("utils/helper.lua"), None);
    assert_eq!(
        f.rows(),
        [
            ("modules/A.lua".to_owned(), "sa2".to_owned()),
            ("modules/B.lua".to_owned(), "sb1".to_owned()),
        ]
    );

    // The new ETag is stored: the next run is a 304.
    f.github.clear_requests();
    f.updater.sync().unwrap();
    assert_eq!(f.github.requests().len(), 1);
}

#[test]
fn a_module_failing_init_after_an_update_is_reported_once_and_its_last_good_version_stays() {
    let f = Fixture::new();
    publish_first(&f.github);
    f.updater.sync().unwrap();
    let before = f.modules.current().get("a").unwrap().clone();
    let broken = "function Init( -- syntax error";
    let b = module("b", "B1");
    f.github.publish(
        "c2",
        "\"e2\"",
        &[
            ("modules/A.lua", "sa2", broken),
            ("modules/B.lua", "sb1", &b),
            ("utils/helper.lua", "sh1", "return {}"),
        ],
    );

    f.updater.sync().unwrap();

    let events = f.db.events().list(&EventQuery::default()).unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].title, "module A.lua failed Init");
    assert_eq!(events[0].kind, "module_update");
    assert_eq!(events[0].severity, EventSeverity::Error);
    assert_eq!(events[0].module_id.as_deref(), Some("a"));
    let after = f.modules.current();
    assert!(Arc::ptr_eq(after.get("a").unwrap(), &before));
    assert!(after.get("b").is_some());
    // The file goes back to the loaded version, so a worker rebuilding its state runs it.
    assert_eq!(f.read("modules/A.lua"), Some(module("a", "A1")));

    // The same broken file in a later commit is not reported again.
    f.github.publish(
        "c3",
        "\"e3\"",
        &[
            ("modules/A.lua", "sa2", broken),
            ("modules/B.lua", "sb1", &b),
            ("utils/helper.lua", "sh1", "return {}"),
        ],
    );
    f.updater.sync().unwrap();
    assert_eq!(f.db.events().list(&EventQuery::default()).unwrap().len(), 1);
    assert!(Arc::ptr_eq(f.modules.current().get("a").unwrap(), &before));
}

#[test]
fn after_an_update_the_worker_pool_runs_the_new_version_of_a_module() {
    let mut f = Fixture::new();
    let pool = Arc::new(WorkerPool::new(pool_config(&f)).unwrap());
    f.updater = f.updater.with_pool(pool.clone());
    publish_first(&f.github);
    f.updater.sync().unwrap();
    let title = || {
        let a = f.modules.current().get("a").unwrap().clone();
        pool.on(&a).get_info("/m").wait().unwrap().value.info.title
    };
    assert_eq!(title(), "A1");
    f.github.publish(
        "c2",
        "\"e2\"",
        &[
            ("modules/A.lua", "sa2", &module("a", "A2")),
            ("modules/B.lua", "sb1", &module("b", "B1")),
            ("utils/helper.lua", "sh1", "return {}"),
        ],
    );

    f.updater.sync().unwrap();

    assert_eq!(title(), "A2");
}

fn pool_config(f: &Fixture) -> PoolConfig {
    let mut config = PoolConfig::new(HttpClient::with_transport(f.github.clone()).unwrap());
    config.threads = 1;
    config.lua_dir = f.lua_dir();
    config
}

#[test]
fn a_module_referencing_unknown_host_api_names_is_reported_once() {
    let f = Fixture::new();
    publish_first(&f.github);
    f.updater.sync().unwrap();
    let uses_unknown = format!(
        "{}function GetNameAndLink() local s = require 'fmd.nosuchlib'; MODULE.NoSuchMember() end\n",
        module("a", "A2")
    );
    let b = module("b", "B1");
    let publish = |commit: &str, etag: &str| {
        f.github.publish(
            commit,
            etag,
            &[
                ("modules/A.lua", "sa2", &uses_unknown),
                ("modules/B.lua", "sb1", &b),
                ("utils/helper.lua", "sh1", "return {}"),
            ],
        );
    };
    publish("c2", "\"e2\"");

    f.updater.sync().unwrap();

    let events = f.db.events().list(&EventQuery::default()).unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].title, "module A.lua uses unknown Host API names");
    assert_eq!(events[0].severity, EventSeverity::Warning);
    assert_eq!(events[0].module_id.as_deref(), Some("a"));
    assert_eq!(
        events[0].body["names"],
        json!(["MODULE.NoSuchMember", "fmd.nosuchlib"])
    );
    // The module still loads.
    assert!(f.modules.current().get("a").is_some());

    // Re-downloading the same version does not report it again.
    std::fs::remove_file(f.lua_dir().join("modules/A.lua")).unwrap();
    publish("c3", "\"e3\"");
    f.updater.sync().unwrap();
    assert_eq!(f.db.events().list(&EventQuery::default()).unwrap().len(), 1);
}

#[test]
fn a_failed_download_keeps_the_old_file_and_is_retried_next_run() {
    let f = Fixture::new();
    publish_first(&f.github);
    f.updater.sync().unwrap();
    f.github.clear_requests();
    let a2 = module("a", "A2");
    let b2 = module("b", "B2");
    f.github.publish(
        "c2",
        "\"e2\"",
        &[
            ("modules/A.lua", "sa2", &a2),
            ("modules/B.lua", "sb2", &b2),
            ("utils/helper.lua", "sh1", "return {}"),
        ],
    );
    f.github.set_unavailable(&["modules/B.lua"]);

    let report = f.updater.sync().unwrap();

    assert_eq!(report.downloaded, ["modules/A.lua"]);
    assert_eq!(report.failed, ["modules/B.lua"]);
    assert_eq!(f.read("modules/A.lua").as_deref(), Some(a2.as_str()));
    assert_eq!(
        f.read("modules/B.lua").as_deref(),
        Some(module("b", "B1").as_str())
    );
    assert_eq!(
        f.rows(),
        [
            ("modules/A.lua".to_owned(), "sa2".to_owned()),
            ("modules/B.lua".to_owned(), "sb1".to_owned()),
            ("utils/helper.lua".to_owned(), "sh1".to_owned()),
        ]
    );

    f.github.set_unavailable(&[]);
    f.github.clear_requests();
    let report = f.updater.sync().unwrap();

    assert_eq!(f.github.downloads(), ["owner/name/c2/lua/modules/B.lua"]);
    assert!(report.failed.is_empty());
    assert_eq!(f.read("modules/B.lua").as_deref(), Some(b2.as_str()));
}

#[test]
fn an_emptied_lua_dir_is_synced_again_in_full() {
    let f = Fixture::new();
    publish_first(&f.github);
    f.updater.sync().unwrap();
    std::fs::remove_dir_all(f.lua_dir()).unwrap();
    f.github.clear_requests();

    f.updater.sync().unwrap();

    let commits = &f.github.requests()[0];
    assert!(
        !commits
            .headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("If-None-Match")),
        "{commits:?}"
    );
    assert_eq!(f.github.downloads().len(), 3);
    assert!(f.modules.current().get("a").is_some());
}

#[test]
fn an_exhausted_rate_limit_stops_api_requests_until_its_reset() {
    let f = Fixture::new();
    publish_first(&f.github);
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 3600;
    f.github.remote.lock().unwrap().rate_limited_until = Some(reset);

    let error = f.updater.sync().unwrap_err();
    assert!(
        matches!(error, UpdateError::RateLimited { reset: r } if r == reset),
        "{error:?}"
    );
    assert_eq!(f.github.requests().len(), 1);

    f.github.clear_requests();
    let error = f.updater.sync().unwrap_err();
    assert!(
        matches!(error, UpdateError::RateLimited { .. }),
        "{error:?}"
    );
    assert!(f.github.requests().is_empty());
    assert!(f.rows().is_empty());
}

#[test]
fn a_token_is_sent_to_the_api_only() {
    let f = Fixture::with(|config| config.token = Some("t0ken".into()));
    publish_first(&f.github);

    f.updater.sync().unwrap();

    for request in f.github.requests() {
        let auth: Vec<_> = request
            .headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("Authorization"))
            .map(|(_, v)| v.as_str())
            .collect();
        if request.url.starts_with(API) {
            assert_eq!(auth, ["Bearer t0ken"], "{}", request.url);
        } else {
            assert!(auth.is_empty(), "{}", request.url);
        }
    }
}

/// Module `a`, whose `GetInfo` makes one request and titles the manga after whether
/// `websitebypass.lua` ran for it.
impl Fixture {
    /// A fixture whose updater invalidates a one-thread pool, and that pool.
    fn pooled() -> (Fixture, Arc<WorkerPool>) {
        let mut f = Fixture::new();
        let pool = Arc::new(WorkerPool::new(pool_config(&f)).unwrap());
        f.updater = f.updater.with_pool(pool.clone());
        (f, pool)
    }

    /// The title `pool` gets from module `a`'s `GetInfo`.
    fn title_on(&self, pool: &WorkerPool) -> String {
        let a = self.modules.current().get("a").unwrap().clone();
        pool.on(&a).get_info("/m").wait().unwrap().value.info.title
    }
}

const BYPASS_PROBE: &str = "function Init() local m = NewWebsiteModule(); m.ID='a'; m.Name='a'; \
     m.RootURL='https://a'; m.OnGetInfo='GetInfo' end\n\
     function GetInfo() BYPASSED = false; HTTP.GET('http://site.test/'); \
     MANGAINFO.Title = BYPASSED and 'bypassed' or 'plain'; return no_error end\n";

/// A `websitebypass.lua` that marks the module state it runs in and fails.
const WEBSITEBYPASS: &str =
    "function ____WebsiteBypass(METHOD, URL) BYPASSED = true; return false end\n";

/// A `checkantibot.lua` whose check answers `seen`.
fn checkantibot(seen: bool) -> String {
    format!("function ____CheckAntiBot(HTTP) return {seen} end\n")
}

#[test]
fn a_synced_checkantibot_is_used_by_the_next_request_without_a_restart() {
    let (f, pool) = Fixture::pooled();
    let publish = |commit: &str, etag: &str, check_sha: &str, seen: bool| {
        f.github.publish(
            commit,
            etag,
            &[
                ("modules/A.lua", "sa1", BYPASS_PROBE),
                (
                    "websitebypass/checkantibot.lua",
                    check_sha,
                    &checkantibot(seen),
                ),
                ("websitebypass/websitebypass.lua", "sw1", WEBSITEBYPASS),
            ],
        );
    };
    publish("c1", "\"e1\"", "sc1", false);
    f.updater.sync().unwrap();
    let title = || f.title_on(&pool);
    assert_eq!(title(), "plain");
    publish("c2", "\"e2\"", "sc2", true);

    f.updater.sync().unwrap();

    // CheckAntiBot now sees a challenge, so `websitebypass.lua` runs
    // (baseunits/lua/LuaWebsiteBypass.pas:160-172).
    assert_eq!(title(), "bypassed");
}

/// Module `a`, whose `GetInfo` makes one request and titles the manga with the FlareSolverr
/// host `websitebypass.lua` read for it.
const CONFIG_PROBE: &str = "function Init() local m = NewWebsiteModule(); m.ID='a'; m.Name='a'; \
     m.RootURL='https://a'; m.OnGetInfo='GetInfo' end\n\
     function GetInfo() SOLVER = nil; HTTP.GET('http://site.test/'); \
     MANGAINFO.Title = SOLVER or 'none'; return no_error end\n";

/// A `websitebypass.lua` that reads the config by the relative Windows path `cloudflare.lua`
/// uses on every bypass (lua/websitebypass/cloudflare.lua:271-325, :341).
const CONFIG_READING_BYPASS: &str = r#"function ____WebsiteBypass(METHOD, URL)
  local f = io.open([[lua\websitebypass\websitebypass_config.json]], 'r')
  if f then SOLVER = f:read('*a'):match('"flaresolverr_ip"%s*:%s*"([^"]*)"'); f:close() end
  return false
end
"#;

#[test]
fn a_flaresolverr_change_is_read_by_the_next_bypass_without_a_restart() {
    let (f, pool) = Fixture::pooled();
    f.github.publish(
        "c1",
        "\"e1\"",
        &[
            ("modules/A.lua", "sa1", CONFIG_PROBE),
            ("websitebypass/checkantibot.lua", "sc1", &checkantibot(true)),
            (
                "websitebypass/websitebypass.lua",
                "sw1",
                CONFIG_READING_BYPASS,
            ),
        ],
    );
    f.updater.sync().unwrap();
    let title = || f.title_on(&pool);
    write_websitebypass_config(&f.lua_dir(), "http://solver-a:8191").unwrap();
    assert_eq!(title(), "solver-a");

    write_websitebypass_config(&f.lua_dir(), "http://solver-b:8191").unwrap();

    assert_eq!(title(), "solver-b");
}

#[test]
fn a_broken_module_from_a_sync_is_never_loaded_by_a_concurrent_job() {
    let (f, pool) = Fixture::pooled();
    publish_first(&f.github);
    f.updater.sync().unwrap();
    // Its `Init` signals that the update is being loaded, holds it there until released, then
    // fails; a state built from it titles every manga `BROKEN`.
    let started = f.dir.path().join("started");
    let release = f.dir.path().join("release");
    let broken = format!(
        "function Init()\n\
           local s = io.open([[{}]], 'w'); s:close()\n\
           while not io.open([[{}]], 'r') do end\n\
           error('broken')\n\
         end\n\
         function GetInfo() MANGAINFO.Title = 'BROKEN'; return no_error end\n",
        started.display(),
        release.display()
    );
    f.github.publish(
        "c2",
        "\"e2\"",
        &[
            ("modules/A.lua", "sa2", &broken),
            ("modules/B.lua", "sb1", &module("b", "B1")),
            ("utils/helper.lua", "sh1", "return {}"),
        ],
    );

    let title = std::thread::scope(|scope| {
        let sync = scope.spawn(|| f.updater.sync().unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !started.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // The pool has never run `a`, so its worker compiles the module file now.
        let a = f.modules.current().get("a").unwrap().clone();
        let title = pool
            .on(&a)
            .get_info("/m")
            .wait()
            .map(|r| r.value.info.title);
        std::fs::write(&release, "").unwrap();
        sync.join().unwrap();
        title
    });

    assert!(started.exists(), "the update's Init never ran");
    assert_eq!(title.unwrap(), "A1");
    assert_eq!(f.read("modules/A.lua"), Some(module("a", "A1")));
}
