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
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("fmd2r"))
        .args(["serve", "--bind", "127.0.0.1:0", "--data-dir"])
        .arg(dir.path().join("data"))
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
