//! FMD2r's stand-in for upstream's `lua/websitebypass/cloudflare.py`, which
//! `lua/websitebypass/cloudflare.lua` runs as `python lua\websitebypass\cloudflare.py <url>
//! --flaresolverr-ip <ip> --flaresolverr-port <port>` (:166-183, :343-344) to get Cloudflare
//! cookies from FlareSolverr.
//!
//! The script cannot run on Linux: it locates its folder as `Path(__file__) / '..'`
//! (lua/websitebypass/cloudflare.py:17-21), which only Windows resolves when `__file__` is a
//! file, so writing `temp_cloudflare.json` there fails before it prints anything (:76). So
//! [`SystemSpawner`](super::subprocess::SystemSpawner) answers that command itself, speaking
//! FlareSolverr's protocol the way the script does and printing the same JSON.
//!
//! It talks to FlareSolverr over a plain `TcpStream` rather than `fmd-http`, like the separate
//! process it replaces: a module session would add FMD2's retries, default headers and cookie
//! handling, and the spawner has no `HttpClient` to hand.
//!
//! Not reproduced: the `rookiepy` fallback that reads cookies out of local desktop browsers
//! (:127-162; reported as not installed, as without the package), the `--testing` re-check of
//! the cookies (:164-189) and the `--debug` log and screenshot files (:26-33, :99-107).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::subprocess::Output;

/// The FlareSolverr session the script always uses (lua/websitebypass/cloudflare.py:39).
const SESSION_ID: &str = "e69e9ce7-8118-470c-a069-13af7bee6e8b";

/// How long to wait for FlareSolverr to connect, and then to answer: solving takes up to its
/// own `maxTimeout`, 60 s by default.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(180);

/// When the FlareSolverr session may next be cleared; the script keeps this in
/// `temp_cloudflare.json` (lua/websitebypass/cloudflare.py:53-78).
static NEXT_SESSION_CLEAR: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether `program args...` runs upstream's `cloudflare.py` with Python.
pub(super) fn is_cloudflare_py(program: &str, args: &[String]) -> bool {
    let name = program
        .rsplit('/')
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    let python = name == "py" || name.starts_with("python");
    python
        && args
            .first()
            .is_some_and(|script| script.ends_with("websitebypass/cloudflare.py"))
}

/// Runs the script's `__main__` (lua/websitebypass/cloudflare.py:228-266) on its arguments
/// after the script path, printing its result as JSON on stdout.
pub(super) fn run(args: &[String]) -> Output {
    let Some(args) = Args::parse(args) else {
        // argparse prints the usage and exits with 2 (lua/websitebypass/cloudflare.py:259).
        return Output {
            stdout: Vec::new(),
            stderr: b"usage: cloudflare.py [-h] [-d] [-t] [-Fip FLARESOLVERR_IP] [-Fp FLARESOLVERR_PORT] Url\n".to_vec(),
            status: 2,
        };
    };
    let solver = Solver {
        host: args.ip,
        port: args.port,
        debug: args.debug,
    };
    Output {
        stdout: format!("{}\n", solver.solve(&args.url, args.testing)).into_bytes(),
        stderr: Vec::new(),
        status: 0,
    }
}

/// The script's command line (`parse_arguments`, lua/websitebypass/cloudflare.py:228-259).
struct Args {
    url: String,
    debug: bool,
    testing: bool,
    ip: String,
    port: String,
}

impl Args {
    fn parse(args: &[String]) -> Option<Args> {
        let mut parsed = Args {
            url: String::new(),
            debug: false,
            testing: false,
            ip: "localhost".into(),
            port: "8191".into(),
        };
        let mut url = None;
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) if name.starts_with('-') => (name, Some(value.to_owned())),
                _ => (arg.as_str(), None),
            };
            match name {
                "-d" | "--debug" => parsed.debug = true,
                "-t" | "--testing" => parsed.testing = true,
                "-Fip" | "--flaresolverr-ip" => {
                    parsed.ip = inline.or_else(|| args.next().cloned())?
                }
                "-Fp" | "--flaresolverr-port" => {
                    parsed.port = inline.or_else(|| args.next().cloned())?
                }
                _ if url.is_none() && !arg.starts_with('-') => url = Some(arg.clone()),
                _ => return None,
            }
        }
        parsed.url = url?;
        Some(parsed)
    }
}

/// `CloudSolver` (lua/websitebypass/cloudflare.py:23-46).
struct Solver {
    host: String,
    port: String,
    debug: bool,
}

impl Solver {
    /// `solve` (lua/websitebypass/cloudflare.py:191-226).
    fn solve(&self, url: &str, testing: bool) -> Value {
        let (status, result) = self.solve_flare(url);
        // The solution is dropped and the cookies passed on as a JSON string (:222-225).
        let result = match result {
            Value::Object(cookies) => Value::String(Value::Object(cookies).to_string()),
            message => message,
        };
        json!({
            "flaresolver_status_code": status,
            "flaresolver_result": result,
            "rookiepy_status_code": 400,
            "rookiepy_result": "Please make sure rookiepy is installed. {use: pip install rookiepy}",
            "testing_cookies": testing,
            "testing_cookies_result": "",
            "enable_debug": self.debug,
        })
    }

    /// `solve_flare` (lua/websitebypass/cloudflare.py:79-125): checks FlareSolverr is ready,
    /// clears its session hourly, then has it load `url`. Returns the status and either the
    /// cookies with `user_agent` or FlareSolverr's message.
    fn solve_flare(&self, url: &str) -> (u16, Value) {
        let not_running = || (404, Value::from("FlareSolverr is not running!"));
        match self.call("GET", "/", None) {
            Ok((200, body)) if body["msg"] != "FlareSolverr is ready!" => return not_running(),
            Ok(_) => {}
            Err(_) => return not_running(),
        }
        self.clear_session();
        let mut command = json!({"cmd": "request.get", "session": SESSION_ID, "url": url});
        let key = if self.debug {
            "returnScreenshot"
        } else {
            "returnOnlyCookies"
        };
        command[key] = Value::Bool(true);
        let (status, answer) = match self.call("POST", "/v1", Some(&command)) {
            Ok(answer) => answer,
            Err(e) => return (500, Value::from(e.to_string())),
        };
        let solution = &answer["solution"];
        if status != 200 || answer["status"] != "ok" || !solution.is_object() {
            return (status, answer["message"].clone());
        }
        let mut result = Map::new();
        for cookie in solution["cookies"].as_array().into_iter().flatten() {
            if let Some(name) = cookie["name"].as_str() {
                result.insert(name.to_owned(), cookie["value"].clone());
            }
        }
        result.insert("user_agent".into(), solution["userAgent"].clone());
        (status, Value::Object(result))
    }

    /// `try_to_clear_sessions` (lua/websitebypass/cloudflare.py:48-78): at most once an hour,
    /// destroys the script's FlareSolverr session so the next solve starts a fresh browser.
    fn clear_session(&self) {
        let mut next = NEXT_SESSION_CLEAR.lock().unwrap_or_else(|e| e.into_inner());
        if next.is_some_and(|next| Instant::now() < next) {
            return;
        }
        if let Ok((_, list)) = self.call("POST", "/v1", Some(&json!({"cmd": "sessions.list"})))
            && list["sessions"]
                .as_array()
                .is_some_and(|s| s.iter().any(|s| s == SESSION_ID))
        {
            let destroy = json!({"cmd": "sessions.destroy", "session": SESSION_ID});
            let _ = self.call("POST", "/v1", Some(&destroy));
        }
        *next = Some(Instant::now() + Duration::from_secs(3600));
    }

    /// One HTTP/1.1 exchange with FlareSolverr; returns the status and the JSON body (`null`
    /// when it is not JSON).
    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> std::io::Result<(u16, Value)> {
        let port: u16 = self.port.trim().parse().map_err(std::io::Error::other)?;
        let address = std::net::ToSocketAddrs::to_socket_addrs(&(self.host.as_str(), port))?
            .next()
            .ok_or_else(|| std::io::Error::other("no address"))?;
        let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
        let body = body.map(Value::to_string).unwrap_or_default();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}:{port}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host,
            body.len()
        )?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        parse_response(&response).ok_or_else(|| std::io::Error::other("bad HTTP response"))
    }
}

/// The status and JSON body of a raw HTTP/1.1 response, with a chunked body decoded.
fn parse_response(response: &[u8]) -> Option<(u16, Value)> {
    let split = response.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&response[..split]).ok()?;
    let mut body = response[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let chunked = lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("transfer-encoding")
                && value.trim().eq_ignore_ascii_case("chunked")
        })
    });
    if chunked {
        body = dechunk(&body)?;
    }
    Some((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// Decodes a chunked transfer encoding.
fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let end = body.windows(2).position(|w| w == b"\r\n")?;
        let size = std::str::from_utf8(&body[..end]).ok()?;
        let size = usize::from_str_radix(size.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        let chunk = body.get(end + 2..end + 2 + size)?;
        out.extend_from_slice(chunk);
        body = body.get(end + 4 + size..)?;
    }
}
