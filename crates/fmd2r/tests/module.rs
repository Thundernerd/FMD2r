//! `fmd2r module init|info|pages` against a fixture Lua dir and a local HTTP server, with HTTP
//! record/replay.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use assert_cmd::Command;
use serde_json::{Value, json};

fn fmd2r() -> Command {
    Command::cargo_bin("fmd2r").unwrap()
}

/// The fixture module, declared with `root_url` as its `RootURL`.
fn fixture_module(root_url: &str) -> String {
    format!(
        r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'fixture'
  m.Name = 'Fixture'
  m.RootURL = '{root_url}'
  m.OnGetInfo = 'GetInfo'
  m.OnTaskStart = 'TaskStart'
  m.OnGetPageNumber = 'GetPageNumber'
  m.OnGetImageURL = 'GetImageURL'
  m.AddOptionCheckBox('showall', 'Show all', false)
end

function GetInfo()
  if not HTTP.GET(MANGAINFO.URL) then return net_problem end
  local x = CreateTXQuery(HTTP.Document)
  MANGAINFO.Title = x.XPathString('//h1')
  x.XPathHREFAll('//li/a', MANGAINFO.ChapterLinks, MANGAINFO.ChapterNames)
  -- A request after a response clears the document (baseunits/httpsendthread.pas:924-946).
  HTTP.Reset()
  if HTTP.POST(MODULE.RootURL .. '/api', 'q=1') then
    MANGAINFO.Genres = HTTP.Document.ToString()
  end
  HTTP.Reset()
  if HTTP.GET(MODULE.RootURL .. '/cover.bin') then
    local s, bytes = HTTP.Document.ToString(), {{}}
    for i = 1, #s do bytes[#bytes + 1] = tostring(s:byte(i)) end
    MANGAINFO.Summary = table.concat(bytes, ',')
  end
  return no_error
end

function TaskStart()
  TASK.PageContainerLinks.Add('started')
  return true
end

function GetPageNumber()
  if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL)) then return false end
  TASK.PageNumber = CreateTXQuery(HTTP.Document).XPathCount('//div[@class="page"]')
  return true
end

function GetImageURL()
  if not HTTP.GET(MaybeFillHost(MODULE.RootURL, URL) .. '/' .. (WORKID + 1)) then return false end
  TASK.PageLinks[WORKID] = CreateTXQuery(HTTP.Document).XPathString('//img/@src')
  return true
end
"#
    )
}

/// One request the test server received.
struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

/// One response of the test server: status, headers and body.
type Response = (u16, Vec<(&'static str, String)>, Vec<u8>);

/// A local HTTP server answering with `route`, one connection per request; stops when dropped.
struct Server {
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Server {
    fn start(route: fn(&Request) -> Response) -> Server {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_owned();
                let path = parts.next().unwrap_or_default().to_owned();
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
                let (status, headers, body) = route(&Request { method, path, body });
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        Server { port, stop }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

const MANGA_HTML: &str = r#"<html><body><h1>Fixture Manga</h1><ul>
<li><a href="/chapter/2">Chapter 2</a></li>
<li><a href="/chapter/1">Chapter 1</a></li>
</ul></body></html>"#;

/// A binary body: NULs, high bytes and a gzip magic that must not be decoded.
const COVER: &[u8] = &[0, 255, 0x1f, 0x8b, 8, 0, 13, 10];

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn brotli(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    brotli::BrotliCompress(&mut &data[..], &mut out, &Default::default()).unwrap();
    out
}

/// The fixture site: the manga page gzip-encoded, the API brotli-encoded, the cover binary.
fn site(request: &Request) -> Response {
    let html = || ("Content-Type", "text/html".to_owned());
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/manga/1") => (
            200,
            vec![html(), ("Content-Encoding", "gzip".into())],
            gzip(MANGA_HTML.as_bytes()),
        ),
        ("POST", "/api") if request.body == b"q=1" => (
            200,
            vec![("Content-Encoding", "br".into())],
            brotli(b"Action, Comedy"),
        ),
        ("GET", "/chapter/1") => (
            200,
            vec![html()],
            br#"<div class="page"></div><div class="page"></div>"#.to_vec(),
        ),
        ("GET", "/chapter/1/1") => (200, vec![html()], br#"<img src="/img/a.jpg">"#.to_vec()),
        ("GET", "/chapter/1/2") => (200, vec![html()], br#"<img src="/img/b.jpg">"#.to_vec()),
        ("GET", "/cover.bin") => (
            200,
            vec![("Content-Type", "image/png".into())],
            COVER.into(),
        ),
        _ => (404, vec![], b"not found".to_vec()),
    }
}

/// A `lua/` dir whose `modules/` holds `files` (name, source).
fn lua_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let modules = dir.path().join("modules");
    std::fs::create_dir(&modules).unwrap();
    for (name, source) in files {
        std::fs::write(modules.join(name), source).unwrap();
    }
    dir
}

fn stdout_json(cmd: &mut Command) -> Value {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "fmd2r failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// `fmd2r module <args[0]> --lua-dir <lua> <args[1..]>`.
fn module_cmd(lua: &Path, args: &[&str]) -> Command {
    let mut cmd = fmd2r();
    cmd.arg("module")
        .args(&args[..1])
        .arg("--lua-dir")
        .arg(lua)
        .args(&args[1..]);
    cmd
}

#[test]
fn init_json_lists_the_fixture_module() {
    let lua = lua_dir(&[("Fixture.lua", &fixture_module("HTTP://Example.COM"))]);
    let out = stdout_json(&mut module_cmd(lua.path(), &["init", "--json"]));
    // `RootURL` is lowercased after `Init` (baseunits/lua/LuaWebsiteModules.pas:553).
    assert_eq!(
        out["modules"],
        json!([{
            "id": "fixture",
            "name": "Fixture",
            "root_url": "http://example.com",
            "callbacks": ["OnGetInfo", "OnTaskStart", "OnGetPageNumber", "OnGetImageURL"],
            "options": [{"name": "showall", "caption": "Show all", "kind": "checkbox", "default": false}],
        }])
    );
    assert_eq!(out["failures"], json!([]));
}

#[test]
fn init_fails_with_the_report_when_a_module_file_does_not_load() {
    let lua = lua_dir(&[
        ("Broken.lua", "function Init() error('boom') end"),
        ("Fixture.lua", &fixture_module("http://example.com")),
    ]);
    let output = module_cmd(lua.path(), &["init"]).output().unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("fixture"), "{stdout}");
    assert!(stderr.contains("Broken.lua"), "{stderr}");
    assert!(stderr.contains("boom"), "{stderr}");
    assert!(stderr.contains("1 of 2 module files failed"), "{stderr}");
}

#[test]
fn init_loads_one_file_or_one_module_when_asked() {
    let other = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'other'
  m.Name = 'Other'
  m.RootURL = 'https://other.org'
end
"#;
    let lua = lua_dir(&[
        ("Fixture.lua", &fixture_module("http://example.com")),
        ("Other.lua", other),
        ("Broken.lua", "function Init() error('boom') end"),
    ]);
    let ids = |out: Value| -> Vec<String> {
        out["modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let file = lua.path().join("modules/Other.lua");
    let mut cmd = module_cmd(lua.path(), &["init", "--json", "--file"]);
    cmd.arg(&file);
    assert_eq!(ids(stdout_json(&mut cmd)), ["other"]);

    // A module given by ID loads every file, so another file failing still fails the run.
    let output = module_cmd(lua.path(), &["init", "--json", "--module", "fixture"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let out: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(ids(out), ["fixture"]);

    let output = module_cmd(lua.path(), &["init", "--module", "nope"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("no module with ID nope"));
}

#[test]
fn info_runs_on_get_info_on_the_module_found_by_host() {
    let server = Server::start(site);
    let lua = lua_dir(&[("Fixture.lua", &fixture_module(&server.url("")))]);
    let out = stdout_json(&mut module_cmd(
        lua.path(),
        &["info", &server.url("/manga/1")],
    ));
    assert_eq!(
        out,
        json!({
            "module": "fixture",
            "status": 0,
            "info": {
                "url": server.url("/manga/1"),
                "title": "Fixture Manga",
                "alt_titles": "",
                "link": "",
                "cover_link": "",
                "authors": "",
                "artists": "",
                "genres": "Action, Comedy",
                "status": "",
                "summary": "0,255,31,139,8,0,13,10",
                "chapter_names": ["Chapter 2", "Chapter 1"],
                "chapter_links": ["/chapter/2", "/chapter/1"],
            },
        })
    );
}

#[test]
fn info_replays_what_it_recorded_with_the_server_stopped() {
    let server = Server::start(site);
    let url = server.url("/manga/1");
    let lua = lua_dir(&[("Fixture.lua", &fixture_module(&server.url("")))]);
    let fixtures = tempfile::tempdir().unwrap();
    let record = fixtures.path().join("R");
    let mut cmd = module_cmd(lua.path(), &["info", &url, "--record"]);
    cmd.arg(&record);
    let recorded = stdout_json(&mut cmd);
    assert_eq!(recorded["info"]["genres"], "Action, Comedy");
    assert_eq!(recorded["info"]["summary"], "0,255,31,139,8,0,13,10");
    drop(server);

    let mut cmd = module_cmd(lua.path(), &["info", &url, "--replay"]);
    cmd.arg(&record);
    assert_eq!(stdout_json(&mut cmd), recorded);
}

#[test]
fn replay_fails_naming_a_request_it_has_no_exchange_for() {
    let server = Server::start(site);
    let lua = lua_dir(&[("Fixture.lua", &fixture_module(&server.url("")))]);
    let fixtures = tempfile::tempdir().unwrap();
    let mut cmd = module_cmd(lua.path(), &["info", &server.url("/manga/1"), "--record"]);
    cmd.arg(fixtures.path());
    stdout_json(&mut cmd);
    drop(server);

    let unrecorded = server_url_of(&lua, "/manga/2");
    let mut cmd = module_cmd(lua.path(), &["info", &unrecorded, "--replay"]);
    let output = cmd.arg(fixtures.path()).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("no recorded exchange for GET {unrecorded}")),
        "{stderr}"
    );
}

/// `path` on the root URL the fixture module in `lua` declares.
fn server_url_of(lua: &tempfile::TempDir, path: &str) -> String {
    let out = stdout_json(&mut module_cmd(lua.path(), &["init", "--json"]));
    format!("{}{path}", out["modules"][0]["root_url"].as_str().unwrap())
}

#[test]
fn replay_ignores_request_headers_unless_told_to_match_them() {
    let server = Server::start(site);
    let module = format!(
        "{}\n{}",
        fixture_module(&server.url("")),
        r#"
function GetInfo()
  HTTP.Headers.Values['X-Token'] = os.getenv('FIXTURE_TOKEN')
  HTTP.GET(MANGAINFO.URL)
  MANGAINFO.Title = CreateTXQuery(HTTP.Document).XPathString('//h1')
  return no_error
end
"#
    );
    let lua = lua_dir(&[("Fixture.lua", &module)]);
    let url = server.url("/manga/1");
    let fixtures = tempfile::tempdir().unwrap();
    let mut cmd = module_cmd(lua.path(), &["info", &url, "--record"]);
    cmd.arg(fixtures.path()).env("FIXTURE_TOKEN", "a");
    stdout_json(&mut cmd);
    drop(server);

    let replay = |token: &str, extra: &[&str]| {
        let mut cmd = module_cmd(lua.path(), &["info", &url, "--replay"]);
        cmd.arg(fixtures.path())
            .args(extra)
            .env("FIXTURE_TOKEN", token);
        cmd.output().unwrap().status.success()
    };
    assert!(replay("b", &[]));
    assert!(replay("a", &["--match-header", "x-token"]));
    assert!(!replay("b", &["--match-header", "x-token"]));
}

#[test]
fn pages_runs_task_start_page_number_and_image_url_per_page() {
    let server = Server::start(site);
    let lua = lua_dir(&[("Fixture.lua", &fixture_module(&server.url("")))]);
    let fixtures = tempfile::tempdir().unwrap();
    let url = server.url("/chapter/1");
    let mut cmd = module_cmd(lua.path(), &["pages", &url, "--record"]);
    cmd.arg(fixtures.path());
    let out = stdout_json(&mut cmd);
    assert_eq!(
        out,
        json!({
            "module": "fixture",
            "page_number": 2,
            "page_links": ["/img/a.jpg", "/img/b.jpg"],
            "page_container_links": ["started"],
        })
    );
    drop(server);
    let mut cmd = module_cmd(lua.path(), &["pages", &url, "--replay"]);
    cmd.arg(fixtures.path());
    assert_eq!(stdout_json(&mut cmd), out);
}

#[test]
fn pages_leaves_page_links_to_resolve_for_a_dynamic_page_link_module() {
    let server = Server::start(site);
    let module = fixture_module(&server.url("")).replace(
        "  m.OnGetInfo = 'GetInfo'",
        "  m.OnGetInfo = 'GetInfo'\n  m.DynamicPageLink = true",
    );
    let lua = lua_dir(&[("Fixture.lua", &module)]);
    let out = stdout_json(&mut module_cmd(
        lua.path(),
        &["pages", &server.url("/chapter/1")],
    ));
    // Unresolved pages are 'W' (baseunits/uDownloadsManager.pas:845-849); a dynamic-page-link
    // module resolves them while downloading.
    assert_eq!(out["page_links"], json!(["W", "W"]));
}

#[test]
fn replay_names_an_unrecorded_request_even_when_the_callback_then_raises() {
    let server = Server::start(site);
    let module = format!(
        "{}\n{}",
        fixture_module(&server.url("")),
        r#"
function GetInfo()
  if not HTTP.GET(MANGAINFO.URL) then error('no page') end
  return no_error
end
"#
    );
    let lua = lua_dir(&[("Fixture.lua", &module)]);
    let fixtures = tempfile::tempdir().unwrap();
    let mut cmd = module_cmd(lua.path(), &["info", &server.url("/manga/1"), "--record"]);
    cmd.arg(fixtures.path());
    stdout_json(&mut cmd);
    let unrecorded = server.url("/manga/2");
    drop(server);

    let mut cmd = module_cmd(lua.path(), &["info", &unrecorded, "--replay"]);
    let output = cmd.arg(fixtures.path()).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("no recorded exchange for GET {unrecorded}")),
        "{stderr}"
    );
}

#[test]
fn info_without_on_get_info_reports_information_not_found() {
    let module = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'bare'
  m.Name = 'Bare'
  m.RootURL = 'https://bare.org'
end
"#;
    let lua = lua_dir(&[("Bare.lua", module)]);
    let out = stdout_json(&mut module_cmd(
        lua.path(),
        &["info", "https://bare.org/manga/1"],
    ));
    // `GetInfoFromURL` without `OnGetInfo` (baseunits/uData.pas:103-109).
    assert_eq!(out["status"], 2);
}

#[test]
fn pages_skips_on_get_page_number_when_task_start_found_the_pages() {
    let module = r#"
function Init()
  local m = NewWebsiteModule()
  m.ID = 'pre'
  m.Name = 'Pre'
  m.RootURL = 'https://pre.org'
  m.OnTaskStart = 'TaskStart'
  m.OnGetPageNumber = 'GetPageNumber'
  m.OnGetImageURL = 'GetImageURL'
end
function TaskStart()
  TASK.PageLinks.Add(' /a.jpg ')
  TASK.PageLinks.Add('')
  return true
end
function GetPageNumber() error('must not run') end
function GetImageURL()
  TASK.PageLinks[WORKID] = ' '
  return true
end
"#;
    let lua = lua_dir(&[("Pre.lua", module)]);
    let out = stdout_json(&mut module_cmd(
        lua.path(),
        &["pages", "https://pre.org/chapter/1"],
    ));
    // `DoGetPageNumber` only runs without page links (baseunits/uDownloadsManager.pas:1181);
    // a page link a module left blank is unresolved again (:1236-1246).
    assert_eq!(out["page_links"], json!([" /a.jpg ", "W"]));
}
