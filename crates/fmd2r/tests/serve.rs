//! `fmd2r serve` against a real socket: starts, answers, and shuts down on SIGINT/SIGTERM.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    addr: String,
    /// What the server logged before it listened.
    startup: Vec<String>,
    _dir: tempfile::TempDir,
}

/// Starts `fmd2r serve` on a free port and waits for its "listening on" line.
fn start() -> Server {
    start_with(&[])
}

/// [`start`] with extra arguments.
fn start_with(args: &[&str]) -> Server {
    let mut all = vec!["--bind", "127.0.0.1:0"];
    all.extend_from_slice(args);
    start_in(tempfile::tempdir().unwrap(), &all)
}

/// Starts `fmd2r serve` with `args` and the data dir `<dir>/data`, and no `FMD2R_*` variables
/// from the test's environment, and waits for its "listening on" line.
fn start_in(dir: tempfile::TempDir, args: &[&str]) -> Server {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("fmd2r"))
        .args(["serve", "--data-dir"])
        .arg(dir.path().join("data"))
        // Tests never reach the network: no module sync with GitHub.
        .arg("--no-module-updates")
        .args(args)
        .env("RUST_LOG", "info")
        .env_remove("FMD2R_BIND")
        .env_remove("FMD2R_PASSWORD")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let mut startup = Vec::new();
    let addr = loop {
        let line = lines
            .next()
            .expect("server exited before listening")
            .unwrap();
        if let Some(rest) = line.split("listening on ").nth(1) {
            break rest.split_whitespace().next().unwrap().to_string();
        }
        startup.push(line);
    };
    // Keep draining stderr so the server never blocks on a full pipe.
    std::thread::spawn(move || lines.for_each(drop));
    Server {
        child,
        addr,
        startup,
        _dir: dir,
    }
}

/// A temp dir whose `data/app.db` has the `server.bind` setting `bind`.
fn dir_with_bind_setting(bind: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    let db = fmd_store::AppDb::open(dir.path().join("data/app.db")).unwrap();
    db.settings()
        .set("server", &serde_json::json!({ "bind": bind }))
        .unwrap();
    dir
}

/// A port nothing listens on right now.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn request(addr: &str, path: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    stream
}

fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    String::from_utf8(head).unwrap()
}

fn signal_and_wait(mut server: Server, signal: &str) {
    let status = Command::new("kill")
        .args(["-s", signal, &server.child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = server.child.try_wait().unwrap() {
            assert!(status.success(), "exit status {status}");
            return;
        }
        if Instant::now() > deadline {
            server.child.kill().unwrap();
            panic!("server did not shut down after SIG{signal}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn serve_answers_health_and_stops_on_sigint_with_an_sse_client_connected() {
    let server = start();
    let mut health = request(&server.addr, "/api/health");
    assert!(read_head(&mut health).starts_with("HTTP/1.1 200"));

    let mut events = request(&server.addr, "/api/events");
    assert!(read_head(&mut events).contains("text/event-stream"));

    signal_and_wait(server, "INT");
}

#[test]
fn serve_stops_on_sigterm() {
    let server = start();
    signal_and_wait(server, "TERM");
}

#[test]
fn startup_points_the_cloudflare_bypass_at_flaresolverr() {
    let server = start_with(&["--flaresolverr-url", "http://flaresolverr:8191"]);
    let config = server
        ._dir
        .path()
        .join("data/lua/websitebypass/websitebypass_config.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config).unwrap()).unwrap();
    // The keys lua/websitebypass/cloudflare.lua:309-322 reads.
    assert_eq!(config["use_webdriver"], true);
    assert_eq!(config["flaresolverr_ip"], "flaresolverr");
    assert_eq!(config["flaresolverr_port"], 8191);
    signal_and_wait(server, "TERM");
}

#[test]
fn a_flaresolverr_url_setting_change_rewrites_the_bypass_config_without_a_restart() {
    let server = start();
    let config = server
        ._dir
        .path()
        .join("data/lua/websitebypass/websitebypass_config.json");
    let body = r#"{"connections":{"flaresolverr_url":"http://solver:8191"}}"#;
    let mut stream = TcpStream::connect(&server.addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "PATCH /api/settings HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        server.addr,
        body.len()
    )
    .unwrap();
    assert!(read_head(&mut stream).starts_with("HTTP/1.1 200"));

    // The keys lua/websitebypass/cloudflare.lua:309-322 reads, at its next bypass.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        if written["flaresolverr_ip"] == "solver" {
            assert_eq!(written["use_webdriver"], true);
            break;
        }
        assert!(Instant::now() < deadline, "{written}");
        std::thread::sleep(Duration::from_millis(20));
    }
    signal_and_wait(server, "TERM");
}

#[test]
fn serve_listens_on_the_bind_setting_without_a_flag() {
    let bind = format!("127.0.0.1:{}", free_port());
    let server = start_in(dir_with_bind_setting(&bind), &[]);
    assert_eq!(server.addr, bind);
    signal_and_wait(server, "TERM");
}

#[test]
fn the_bind_flag_wins_over_the_setting() {
    let setting = format!("127.0.0.1:{}", free_port());
    let flag = format!("127.0.0.1:{}", free_port());
    let server = start_in(dir_with_bind_setting(&setting), &["--bind", &flag]);
    assert_eq!(server.addr, flag);
    signal_and_wait(server, "TERM");
}

/// Whether the server warned at startup that anyone who can reach it can use it.
fn warned_open(server: &Server) -> bool {
    server
        .startup
        .iter()
        .any(|line| line.contains("WARN") && line.contains("no password"))
}

#[test]
fn startup_warns_when_any_address_can_reach_an_open_server() {
    let server = start_in(tempfile::tempdir().unwrap(), &["--bind", "0.0.0.0:0"]);
    assert!(warned_open(&server), "{:?}", server.startup);
    signal_and_wait(server, "TERM");
}

#[test]
fn startup_does_not_warn_on_loopback_or_with_a_password() {
    let server = start();
    assert!(!warned_open(&server), "{:?}", server.startup);
    signal_and_wait(server, "TERM");

    let args = ["--bind", "0.0.0.0:0", "--password", "hunter2"];
    let server = start_in(tempfile::tempdir().unwrap(), &args);
    assert!(!warned_open(&server), "{:?}", server.startup);
    signal_and_wait(server, "TERM");
}
