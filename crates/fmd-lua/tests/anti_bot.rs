//! The anti-bot hook (`WebsiteBypassRequest`, baseunits/lua/LuaWebsiteBypass.pas:142-212) on a
//! module's `HTTP` objects, with upstream's `websitebypass/*.lua` (docs/tickets/T30-anti-bot.md).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use fmd_http::{BoxFuture, HttpClient, Transport, TransportError, WireRequest, WireResponse};
use fmd_lua::{
    HttpModule, LuaHttp, Module, ModuleHttpOverrides, ModuleHttpSettings, ModuleRegistry, Runtime,
    SettingsStoreError, create_http,
};
use fmd_testkit::corpus_root;

/// The status, headers and body a stub answers with.
type Answer = (u16, Vec<(String, String)>, Vec<u8>);

/// A transport that records every request and answers through a function of it.
struct StubTransport {
    requests: Mutex<Vec<WireRequest>>,
    answer: Box<dyn Fn(&WireRequest) -> Answer + Send + Sync>,
}

impl StubTransport {
    fn new(answer: impl Fn(&WireRequest) -> Answer + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(StubTransport {
            requests: Mutex::default(),
            answer: Box::new(answer),
        })
    }

    fn requests(&self) -> Vec<WireRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Transport for StubTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let (status, headers, body) = (self.answer)(&request);
        self.requests.lock().unwrap().push(request);
        Box::pin(async move {
            Ok(WireResponse {
                status,
                reason: String::new(),
                headers,
                body,
            })
        })
    }
}

fn header<'a>(request: &'a WireRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Cloudflare's challenge page: what `____CheckAntiBot` looks for
/// (lua/websitebypass/checkantibot.lua:6-9).
fn challenge() -> Answer {
    (
        503,
        vec![
            ("Server".into(), "cloudflare".into()),
            ("Content-Type".into(), "text/html; charset=UTF-8".into()),
        ],
        b"<title>Just a moment...</title>".to_vec(),
    )
}

fn ok(body: &str) -> Answer {
    (
        200,
        vec![("Content-Type".into(), "text/html".into())],
        body.as_bytes().to_vec(),
    )
}

/// A site behind Cloudflare: the challenge until a request carries `cf_clearance=solved`.
fn protected_site(request: &WireRequest) -> Answer {
    match header(request, "Cookie") {
        Some(cookies) if cookies.contains("cf_clearance=solved") => ok("the manga page"),
        _ => challenge(),
    }
}

/// Module settings kept in memory: `Settings.Enabled` and `Settings.HTTP`.
#[derive(Default)]
struct Settings(Mutex<(bool, ModuleHttpOverrides)>);

impl Settings {
    fn enabled(&self) -> bool {
        self.0.lock().unwrap().0
    }

    fn http(&self) -> ModuleHttpOverrides {
        self.0.lock().unwrap().1.clone()
    }
}

impl ModuleHttpSettings for Settings {
    fn http_overrides(&self) -> Option<ModuleHttpOverrides> {
        let (enabled, http) = &*self.0.lock().unwrap();
        enabled.then(|| http.clone())
    }

    fn clear_cookies(&self) -> Result<(), SettingsStoreError> {
        self.0.lock().unwrap().1.cookies.clear();
        Ok(())
    }

    fn store_bypass(&self, cookies: &str, user_agent: &str) -> Result<(), SettingsStoreError> {
        let (enabled, http) = &mut *self.0.lock().unwrap();
        *enabled = true;
        http.cookies = cookies.into();
        http.user_agent = user_agent.into();
        Ok(())
    }
}

/// A `websitebypass.lua` stand-in that solves the challenge at once and counts its calls in
/// `MODULE.Storage['bypass_calls']`.
const SOLVING_BYPASS: &str = r#"
function ____WebsiteBypass(METHOD, URL)
  MODULE.Storage['bypass_calls'] = tostring((tonumber(MODULE.Storage['bypass_calls']) or 0) + 1)
  MODULE.Storage['bypass_args'] = METHOD .. ' ' .. URL
  HTTP.Cookies.Values['cf_clearance'] = 'solved'
  HTTP.UserAgent = 'Solver/1.0'
  MODULE.Storage['reload'] = 'true'
  return true
end
"#;

const MODULE_FILE: &str = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'site'; m.Name = 'Site'; m.RootURL = 'https://site.test'
end
"#;

/// A lua dir with upstream's `checkantibot.lua`, `bypass` as `websitebypass.lua` and one module.
fn lua_dir(bypass: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let bypass_dir = dir.path().join("websitebypass");
    fs::create_dir_all(&bypass_dir).unwrap();
    fs::copy(
        corpus_root().join("websitebypass/checkantibot.lua"),
        bypass_dir.join("checkantibot.lua"),
    )
    .unwrap();
    fs::write(bypass_dir.join("websitebypass.lua"), bypass).unwrap();
    fs::create_dir_all(dir.path().join("modules")).unwrap();
    fs::write(dir.path().join("modules/site.lua"), MODULE_FILE).unwrap();
    dir
}

/// A module's state: its client, settings and loaded `Module`.
struct Site {
    client: HttpClient,
    settings: Arc<Settings>,
    module: Arc<Module>,
}

impl Site {
    fn new(lua_dir: &Path, transport: Arc<StubTransport>) -> Site {
        let report = ModuleRegistry::load_dir(lua_dir);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        Site {
            client: HttpClient::with_transport(transport).unwrap(),
            settings: Arc::default(),
            module: report.registry.get("site").unwrap().clone(),
        }
    }

    /// A runtime set up like a module callback, with the anti-bot hook on `HTTP`.
    fn runtime(&self, lua_dir: &Path) -> Runtime {
        let rt = Runtime::new().unwrap();
        rt.set_lua_dir(lua_dir);
        rt.set_module(&self.module).unwrap();
        let module = HttpModule {
            http: self.module.http().clone(),
            settings: self.settings.clone(),
        };
        let session = create_http(&self.client, Some(&module));
        let http =
            LuaHttp::with_website_bypass(session, self.module.clone(), self.settings.clone())
                .build(rt.lua())
                .unwrap();
        rt.lua().globals().set("HTTP", http).unwrap();
        rt
    }

    fn storage(&self, name: &str) -> String {
        String::from_utf8(self.module.storage_value(name)).unwrap()
    }
}

#[test]
fn a_solved_challenge_stores_cookies_and_user_agent_and_reloads_the_page() {
    let dir = lua_dir(SOLVING_BYPASS);
    let stub = StubTransport::new(protected_site);
    let site = Site::new(dir.path(), stub.clone());

    site.runtime(dir.path())
        .exec(
            r#"
            assert(HTTP.GET('https://site.test/manga/1') == true)
            assert(HTTP.ResultCode == 200, HTTP.ResultCode)
            assert(HTTP.Document.ToString() == 'the manga page')
            "#,
        )
        .unwrap();

    assert_eq!(site.storage("bypass_calls"), "1");
    assert_eq!(site.storage("bypass_args"), "GET https://site.test/manga/1");
    // baseunits/lua/LuaWebsiteBypass.pas:182-184: enabled, `Cookies.Text` with its line breaks
    // turned into `;`, and the session's user agent.
    assert!(site.settings.enabled());
    assert_eq!(site.settings.http().cookies, "cf_clearance=solved;");
    assert_eq!(site.settings.http().user_agent, "Solver/1.0");
    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(header(&requests[1], "User-Agent"), Some("Solver/1.0"));
}

#[test]
fn successful_responses_never_call_the_bypass() {
    let dir = lua_dir(SOLVING_BYPASS);
    let stub = StubTransport::new(|_| ok("fine"));
    let site = Site::new(dir.path(), stub.clone());

    site.runtime(dir.path())
        .exec(
            r#"
            for i = 1, 5 do assert(HTTP.GET('https://site.test/page/' .. i) == true) end
            assert(HTTP.POST('https://site.test/search', 'q=x') == true)
            assert(HTTP.XHR('https://site.test/api') == true)
            assert(HTTP.HEAD('https://site.test/') == true)
            "#,
        )
        .unwrap();

    assert_eq!(site.storage("bypass_calls"), "");
    assert_eq!(stub.requests().len(), 8);
    assert!(!site.settings.enabled());
}

#[test]
fn request_does_not_run_the_hook() {
    // `http_request` calls `HTTPRequest`, not `InternalHTTPRequest`
    // (baseunits/lua/LuaHTTPSend.pas:23, baseunits/httpsendthread.pas:487-495).
    let dir = lua_dir(SOLVING_BYPASS);
    let stub = StubTransport::new(protected_site);
    let site = Site::new(dir.path(), stub.clone());

    site.runtime(dir.path())
        .exec(
            r#"
            HTTP.RetryCount = 0
            HTTP.Request('GET', 'https://site.test/manga/1')
            assert(HTTP.ResultCode == 503)
            "#,
        )
        .unwrap();

    assert_eq!(site.storage("bypass_calls"), "");
}

/// [`SOLVING_BYPASS`] that takes a while, so a second thread meets the challenge meanwhile.
const SLOW_BYPASS: &str = r#"
function ____WebsiteBypass(METHOD, URL)
  MODULE.Storage['bypass_calls'] = tostring((tonumber(MODULE.Storage['bypass_calls']) or 0) + 1)
  local done = os.time() + 1
  while os.time() <= done do end
  HTTP.Cookies.Values['cf_clearance'] = 'solved'
  MODULE.Storage['reload'] = 'true'
  return true
end
"#;

#[test]
fn concurrent_challenges_of_one_module_run_the_bypass_once() {
    let dir = lua_dir(SLOW_BYPASS);
    let stub = StubTransport::new(protected_site);
    let site = Arc::new(Site::new(dir.path(), stub.clone()));
    let start = Arc::new(std::sync::Barrier::new(2));

    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (site, start, dir) = (site.clone(), start.clone(), dir.path().to_path_buf());
            std::thread::spawn(move || {
                let rt = site.runtime(&dir);
                start.wait();
                rt.exec("assert(HTTP.GET('https://site.test/manga/1') == true)")
                    .unwrap();
                rt.eval::<String>("HTTP.Document.ToString()").unwrap()
            })
        })
        .collect();
    let bodies: Vec<String> = threads.into_iter().map(|t| t.join().unwrap()).collect();

    assert_eq!(site.storage("bypass_calls"), "1");
    // The thread that did not bypass waited for the other one and re-sent with its cookie.
    assert_eq!(bodies, ["the manga page", "the manga page"]);
    assert_eq!(stub.requests().len(), 4);
}

#[test]
fn a_failed_bypass_clears_the_stored_cookies_and_the_request_fails() {
    let dir = lua_dir("function ____WebsiteBypass(METHOD, URL) return false end");
    let stub = StubTransport::new(protected_site);
    let site = Site::new(dir.path(), stub.clone());
    site.settings.store_bypass("stale=1;", "Old/1.0").unwrap();

    site.runtime(dir.path())
        .exec(
            r#"
            assert(HTTP.GET('https://site.test/manga/1') == false)
            assert(HTTP.ResultCode == 503)
            "#,
        )
        .unwrap();

    // baseunits/lua/LuaWebsiteBypass.pas:178-179: cleared before the bypass, nothing stored.
    assert_eq!(site.settings.http().cookies, "");
    assert_eq!(site.settings.http().user_agent, "Old/1.0");
    assert_eq!(stub.requests().len(), 1);
}

#[test]
fn a_bypass_that_errors_counts_as_failed() {
    let dir = lua_dir("function ____WebsiteBypass(METHOD, URL) error('boom') end");
    let site = Site::new(dir.path(), StubTransport::new(protected_site));

    site.runtime(dir.path())
        .exec("assert(HTTP.GET('https://site.test/manga/1') == false)")
        .unwrap();

    assert!(!site.settings.enabled());
}

#[test]
fn without_reload_the_bypass_answer_is_the_result() {
    // cloudflare.lua sets `reload` to "false" unless FlareSolverr's page still shows the
    // challenge (lua/websitebypass/cloudflare.lua:363, :383-391).
    let dir = lua_dir(
        r#"
        function ____WebsiteBypass(METHOD, URL)
          HTTP.Cookies.Values['cf_clearance'] = 'solved'
          MODULE.Storage['reload'] = 'false'
          return true
        end
        "#,
    );
    let stub = StubTransport::new(protected_site);
    let site = Site::new(dir.path(), stub.clone());

    site.runtime(dir.path())
        .exec(
            r#"
            assert(HTTP.GET('https://site.test/manga/1') == true)
            assert(HTTP.ResultCode == 503)
            "#,
        )
        .unwrap();

    assert!(site.settings.enabled());
    assert_eq!(site.settings.http().cookies, "cf_clearance=solved;");
    assert_eq!(stub.requests().len(), 1);
}

/// What a FlareSolverr stub received.
#[derive(Default)]
struct FlareSolverrLog {
    /// `(method, path, body)` of every request.
    requests: Vec<(String, String, String)>,
}

/// A loopback FlareSolverr stub solving every `request.get` with cookie `cf_clearance=solved`
/// and user agent `Solver/2.0`. Returns its port.
fn flaresolverr_stub(log: Arc<Mutex<FlareSolverrLog>>) -> u16 {
    use std::io::{BufRead, BufReader, Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let (method, path) = (
                parts.next().unwrap().to_owned(),
                parts.next().unwrap().to_owned(),
            );
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let request: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let answer = match (method.as_str(), request["cmd"].as_str()) {
                ("GET", _) => {
                    serde_json::json!({"msg": "FlareSolverr is ready!", "version": "3.3.21"})
                }
                ("POST", Some("sessions.list")) => {
                    serde_json::json!({"status": "ok", "sessions": []})
                }
                ("POST", Some("request.get")) => serde_json::json!({
                    "status": "ok",
                    "message": "Challenge solved!",
                    "solution": {
                        "url": request["url"],
                        "status": 200,
                        "cookies": [{"name": "cf_clearance", "value": "solved"}],
                        "userAgent": "Solver/2.0"
                    }
                }),
                _ => serde_json::json!({"status": "error", "message": "unknown"}),
            }
            .to_string();
            log.lock().unwrap().requests.push((method, path, body));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
        }
    });
    port
}

#[test]
fn upstream_cloudflare_lua_solves_a_challenge_through_flaresolverr() {
    let root = tempfile::tempdir().unwrap();
    let lua = root.path().join("lua");
    for file in [
        "websitebypass/checkantibot.lua",
        "websitebypass/websitebypass.lua",
        "websitebypass/cloudflare.lua",
        "websitebypass/cloudflare.py",
        "utils/json.lua",
    ] {
        fs::create_dir_all(lua.join(file).parent().unwrap()).unwrap();
        fs::copy(corpus_root().join(file), lua.join(file)).unwrap();
    }
    fs::create_dir_all(lua.join("modules")).unwrap();
    fs::write(lua.join("modules/site.lua"), MODULE_FILE).unwrap();
    let log = Arc::new(Mutex::new(FlareSolverrLog::default()));
    let port = flaresolverr_stub(log.clone());
    fs::write(
        lua.join("websitebypass/websitebypass_config.json"),
        format!(
            r#"{{"use_webdriver": true, "debug": false, "testing": false,
                "flaresolverr_ip": "127.0.0.1", "flaresolverr_port": {port}}}"#
        ),
    )
    .unwrap();
    // The module's own requests, including cloudflare.lua's readiness check of FlareSolverr
    // (lua/websitebypass/cloudflare.lua:346-356), go through the stub transport.
    let flaresolverr = format!("http://127.0.0.1:{port}/");
    let stub = StubTransport::new(move |request| {
        if request.url == flaresolverr {
            ok(r#"{"msg": "FlareSolverr is ready!"}"#)
        } else {
            protected_site(request)
        }
    });
    let site = Site::new(&lua, stub);
    let rt = site.runtime(&lua);
    rt.set_working_dir(root.path());

    rt.exec(
        r#"
        assert(HTTP.GET('https://site.test/manga/1') == true)
        assert(HTTP.ResultCode == 200, HTTP.ResultCode)
        assert(HTTP.Document.ToString() == 'the manga page')
        "#,
    )
    .unwrap();

    assert!(site.settings.enabled());
    assert_eq!(site.settings.http().cookies, "cf_clearance=solved;");
    assert_eq!(site.settings.http().user_agent, "Solver/2.0");
    // FlareSolverr's page no longer shows the challenge, so no reload
    // (lua/websitebypass/cloudflare.lua:383-391).
    assert_eq!(site.storage("reload"), "false");
    let log = log.lock().unwrap();
    let solve = log
        .requests
        .iter()
        .find(|(method, path, body)| {
            method == "POST" && path == "/v1" && body.contains("request.get")
        })
        .expect("a request.get command");
    let command: serde_json::Value = serde_json::from_str(&solve.2).unwrap();
    // cloudflare.py asks for the site's root (lua/websitebypass/cloudflare.lua:167, :174).
    assert_eq!(command["url"], "https://site.test");
}
