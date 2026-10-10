//! Checks for the external tools FMD2r relies on: `python3` and `node` (run by upstream Lua
//! modules), `magick` (image conversion) and a FlareSolverr instance (Cloudflare bypass).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

/// How long one tool check may take before it counts as failed.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// How much of FlareSolverr's answer is read.
const MAX_RESPONSE: u64 = 64 * 1024;

/// The outcome of checking one tool.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ToolCheck {
    pub name: String,
    /// Whether the tool is usable.
    pub ok: bool,
    /// Its version or address when usable, otherwise why not.
    pub detail: String,
}

/// Checks which external tools are available. Blocking; may take a few seconds.
pub trait ToolProbe: Send + Sync + 'static {
    fn probe(&self) -> Vec<ToolCheck>;
}

/// Probes the real system: runs each tool's version command and asks FlareSolverr whether it is
/// ready.
#[derive(Debug, Clone, Default)]
pub struct SystemTools {
    bypass_config: Option<PathBuf>,
}

impl SystemTools {
    /// Reads the FlareSolverr address from `config` (upstream's
    /// `lua/websitebypass/websitebypass_config.json`) at each probe.
    pub fn new(config: impl Into<PathBuf>) -> Self {
        Self {
            bypass_config: Some(config.into()),
        }
    }

    /// FlareSolverr's address: the config's `flaresolverr_ip`/`flaresolverr_port`, defaulting to
    /// `localhost:8191` like upstream (lua/websitebypass/cloudflare.lua:278-279, :315-322).
    fn flaresolverr_addr(&self) -> (String, u16) {
        let config = self
            .bypass_config
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<BypassConfig>(&b).ok());
        let (ip, port) = config.map_or((None, None), |c| (c.flaresolverr_ip, c.flaresolverr_port));
        (
            ip.unwrap_or_else(|| "localhost".into()),
            port.unwrap_or(8191),
        )
    }

    /// Asks FlareSolverr whether it runs the way upstream does: `GET /` answers 200 with
    /// `"msg": "FlareSolverr is ready!"` (lua/websitebypass/cloudflare.lua:346-356).
    fn flaresolverr_check(&self) -> ToolCheck {
        let (host, port) = self.flaresolverr_addr();
        // On its own thread so a slow name lookup cannot hold the check past its deadline; a
        // thread still stuck then ends with its socket timeouts.
        let (tx, rx) = mpsc::channel();
        let (h, deadline) = (host.clone(), Instant::now() + PROBE_TIMEOUT);
        std::thread::spawn(move || {
            let _ = tx.send(flaresolverr_ready(&h, port, deadline));
        });
        let result = rx
            .recv_timeout(PROBE_TIMEOUT + Duration::from_millis(100))
            .unwrap_or(Err(ProbeError::Timeout));
        ToolCheck {
            name: "FlareSolverr".into(),
            ok: result.is_ok(),
            detail: match result {
                Ok(()) => format!("{host}:{port}"),
                Err(e) => format!("{host}:{port}: {e}"),
            },
        }
    }
}

impl ToolProbe for SystemTools {
    fn probe(&self) -> Vec<ToolCheck> {
        std::thread::scope(|scope| {
            let commands = [
                ("python3", "--version"),
                ("node", "--version"),
                ("magick", "-version"),
            ]
            .map(|(name, arg)| scope.spawn(move || version_check(name, arg)));
            let flaresolverr = scope.spawn(|| self.flaresolverr_check());
            commands
                .into_iter()
                .chain([flaresolverr])
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| ToolCheck {
                        name: "?".into(),
                        ok: false,
                        detail: "check crashed".into(),
                    })
                })
                .collect()
        })
    }
}

/// The default, so nothing spawns processes or dials out unless asked to.
pub(crate) struct NoTools;

impl ToolProbe for NoTools {
    fn probe(&self) -> Vec<ToolCheck> {
        Vec::new()
    }
}

/// Why a tool check failed; shown as the check's detail.
#[derive(Debug, Error)]
enum ProbeError {
    #[error("not found on PATH")]
    NotFound,
    #[error("no answer within {}s", PROBE_TIMEOUT.as_secs())]
    Timeout,
    #[error("exited with {status}: {output}")]
    Exited { status: ExitStatus, output: String },
    #[error("address does not resolve")]
    Unresolved,
    #[error("unexpected answer: {0}")]
    UnexpectedAnswer(String),
    #[error("not FlareSolverr, or not ready")]
    NotReady,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Runs `name arg` and reports the first line it prints.
fn version_check(name: &str, arg: &str) -> ToolCheck {
    let result = run_with_timeout(name, arg);
    ToolCheck {
        name: name.into(),
        ok: result.is_ok(),
        detail: result.unwrap_or_else(|e| e.to_string()),
    }
}

fn run_with_timeout(name: &str, arg: &str) -> Result<String, ProbeError> {
    let mut child = Command::new(name)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ProbeError::NotFound,
            _ => e.into(),
        })?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProbeError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Version output is a few lines, far below the pipe buffer, so the child never blocked on it.
    let output = child.wait_with_output()?;
    // Older Pythons print their version on stderr.
    let text = [&output.stdout, &output.stderr]
        .into_iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .find(|t| !t.trim().is_empty())
        .unwrap_or_default();
    let first = text.lines().next().unwrap_or_default().trim().to_owned();
    if output.status.success() {
        Ok(first)
    } else {
        Err(ProbeError::Exited {
            status: output.status,
            output: first,
        })
    }
}

#[derive(Deserialize)]
struct BypassConfig {
    flaresolverr_ip: Option<String>,
    flaresolverr_port: Option<u16>,
}

/// Connects to each address `host` resolves to in turn (`localhost` may resolve to `::1` first
/// while FlareSolverr listens on IPv4 only), all before `deadline`.
fn connect(host: &str, port: u16, deadline: Instant) -> Result<TcpStream, ProbeError> {
    let mut last = ProbeError::Unresolved;
    for addr in (host, port).to_socket_addrs()? {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ProbeError::Timeout);
        }
        match TcpStream::connect_timeout(&addr, left) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e.into(),
        }
    }
    Err(last)
}

fn flaresolverr_ready(host: &str, port: u16, deadline: Instant) -> Result<(), ProbeError> {
    let mut stream = connect(host, port, deadline)?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT))?;
    write!(
        stream,
        "GET / HTTP/1.0\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    )?;
    // Read with the time left before the deadline, so a trickling answer cannot stretch it.
    let mut response = Vec::new();
    let mut chunk = [0u8; 4096];
    while (response.len() as u64) < MAX_RESPONSE {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ProbeError::Timeout);
        }
        stream.set_read_timeout(Some(left))?;
        match stream.read(&mut chunk)? {
            0 => break,
            n => response.extend_from_slice(&chunk[..n]),
        }
    }
    let response = String::from_utf8_lossy(&response);
    let status = response.lines().next().unwrap_or_default();
    if status.split_whitespace().nth(1) != Some("200") {
        return Err(ProbeError::UnexpectedAnswer(status.to_owned()));
    }
    if !response.contains("FlareSolverr is ready!") {
        return Err(ProbeError::NotReady);
    }
    Ok(())
}
