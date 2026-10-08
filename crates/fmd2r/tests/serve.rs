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
    _dir: tempfile::TempDir,
}

/// Starts `fmd2r serve` on a free port and waits for its "listening on" line.
fn start() -> Server {
    start_with(&[])
}

/// [`start`] with extra arguments.
fn start_with(args: &[&str]) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("fmd2r"));
    cmd.args(["serve", "--bind", "127.0.0.1:0", "--data-dir"])
        .arg(dir.path().join("data"))
        .args(args);
    spawn(cmd, dir)
}

/// Runs `cmd` (an `fmd2r serve` command whose data lives in `dir`) and waits for its
/// "listening on" line.
fn spawn(mut cmd: Command, dir: tempfile::TempDir) -> Server {
    let mut child = cmd
        .env("RUST_LOG", "info")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let addr = loop {
        let line = lines
            .next()
            .expect("server exited before listening")
            .unwrap();
        if let Some(rest) = line.split("listening on ").nth(1) {
            break rest.split_whitespace().next().unwrap().to_string();
        }
    };
    // Keep draining stderr so the server never blocks on a full pipe.
    std::thread::spawn(move || lines.for_each(drop));
    Server {
        child,
        addr,
        _dir: dir,
    }
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
fn serve_takes_the_bind_address_and_data_dir_from_the_environment() {
    // The Docker image configures `serve` through these (Dockerfile).
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("from-env");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("fmd2r"));
    cmd.arg("serve")
        .env("FMD2R_BIND", "127.0.0.1:0")
        .env("FMD2R_DATA_DIR", &data);
    let server = spawn(cmd, dir);
    let mut health = request(&server.addr, "/api/health");
    assert!(read_head(&mut health).starts_with("HTTP/1.1 200"));
    assert!(data.join("app.db").is_file());
    signal_and_wait(server, "TERM");
}
