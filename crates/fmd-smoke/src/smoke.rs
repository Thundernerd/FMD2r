//! The smoke list and its runs: `fmd2r module info|pages` on every entry, live, replayed from the
//! recorded fixtures, or recording them.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, io, thread};

use serde::Deserialize;
use serde_json::Value;

use crate::{EntryResult, StepResult};

/// Failure to read or write the smoke list or its fixtures.
#[derive(Debug, thiserror::Error)]
pub enum SmokeError {
    #[error("cannot access {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("cannot parse {}: {source}", path.display())]
    ListParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("cannot parse {}: {source}", path.display())]
    JsonParse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("cannot serialize {what}: {source}")]
    JsonWrite {
        what: &'static str,
        source: serde_json::Error,
    },
    #[error("no smoke entry named {0}")]
    UnknownEntry(String),
    #[error("{name} {step}: {detail}")]
    Record {
        name: String,
        step: Step,
        detail: String,
    },
}

/// An [`SmokeError::Io`] on `path`, for `map_err`.
pub(crate) fn io_error(path: &Path) -> impl FnOnce(io::Error) -> SmokeError + use<> {
    let path = path.to_owned();
    move |source| SmokeError::Io { path, source }
}

/// `fixtures/smoke/list.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct SmokeList {
    #[serde(rename = "entry", default)]
    pub entries: Vec<Entry>,
}

/// One module and the URLs it is run on.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The entry's directory under `fixtures/smoke`.
    pub name: String,
    /// The module's `ID` (not its name: names aren't unique).
    pub module_id: String,
    /// The URL `module info` runs on.
    pub manga_url: String,
    /// The URL `module pages` runs on.
    pub chapter_url: String,
    /// What the entry covers: its template, the libraries it uses, ...
    pub notes: String,
}

/// The two runs of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// `module info <manga_url>`.
    Info,
    /// `module pages <chapter_url>`.
    Pages,
}

impl Step {
    const ALL: [Step; 2] = [Step::Info, Step::Pages];

    fn command(self) -> &'static str {
        match self {
            Step::Info => "info",
            Step::Pages => "pages",
        }
    }

    fn url(self, entry: &Entry) -> &str {
        match self {
            Step::Info => &entry.manga_url,
            Step::Pages => &entry.chapter_url,
        }
    }
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.command())
    }
}

/// How long one `fmd2r module` run may take.
const TIMEOUT: Duration = Duration::from_secs(300);
/// Response bodies of these media types are dropped after recording: `info` and `pages` don't
/// need image data, and they would make the fixtures large.
const TRUNCATED_MEDIA: &str = "image/";

/// Where a step's HTTP goes.
enum Mode {
    /// To the live site.
    Live,
    /// To the live site, recorded into this fixture directory.
    Record(PathBuf),
    /// To the fixtures recorded in this directory, with an empty `PATH`: a module that runs a
    /// program (node, python, ...) through `fmd.subprocess` would reach the network past the
    /// replay.
    Replay(PathBuf),
}

/// Where the smoke list lives and what runs it.
#[derive(Debug, Clone)]
pub struct Smoke {
    /// The `fmd2r` binary.
    pub fmd2r: PathBuf,
    /// The `lua/` tree the modules load from (`fixtures/lua`).
    pub lua_dir: PathBuf,
    /// `fixtures/smoke`: `list.toml` and one directory per entry.
    pub dir: PathBuf,
}

impl Smoke {
    /// Reads `list.toml`.
    pub fn list(&self) -> Result<SmokeList, SmokeError> {
        let path = self.dir.join("list.toml");
        let text = fs::read_to_string(&path).map_err(io_error(&path))?;
        toml::from_str(&text).map_err(|source| SmokeError::ListParse { path, source })
    }

    /// The entry named `name`.
    pub fn entry(&self, name: &str) -> Result<Entry, SmokeError> {
        self.list()?
            .entries
            .into_iter()
            .find(|e| e.name == name)
            .ok_or_else(|| SmokeError::UnknownEntry(name.to_owned()))
    }

    /// Runs the entry on its recorded traffic: each step passes when it prints its snapshot.
    pub fn replay(&self, entry: &Entry) -> EntryResult {
        self.run_steps(entry, |step| {
            let out = self.run(entry, step, Mode::Replay(self.fixtures(entry, step)))?;
            let snapshot_path = self.snapshot(entry, step);
            let snapshot = fs::read_to_string(&snapshot_path)
                .map_err(|e| format!("cannot read {}: {e}", snapshot_path.display()))?;
            compare(&snapshot, &out)
                .map_err(|e| format!("the output differs from the snapshot {e}"))
        })
    }

    /// Runs the entry against the live site: each step passes when it finds something (see
    /// [`check_output`]).
    pub fn live(&self, entry: &Entry) -> EntryResult {
        self.run_steps(entry, |step| {
            let out = self.run(entry, step, Mode::Live)?;
            check_output(step, &out)
        })
    }

    /// Records the entry from the live site, replacing its fixtures and snapshots: each step's
    /// traffic is recorded, image bodies are dropped, and the step is replayed; the replay's
    /// output, which must match the live one, becomes the snapshot.
    pub fn record(&self, entry: &Entry) -> Result<(), SmokeError> {
        for step in Step::ALL {
            let failed = |detail: String| SmokeError::Record {
                name: entry.name.clone(),
                step,
                detail,
            };
            let fixtures = self.fixtures(entry, step);
            let live = self
                .run(entry, step, Mode::Record(fixtures.clone()))
                .map_err(failed)?;
            check_output(step, &live).map_err(failed)?;
            truncate_media(&fixtures)?;
            let replayed = self
                .run(entry, step, Mode::Replay(fixtures))
                .map_err(|e| failed(format!("replaying the recording: {e}")))?;
            compare(&live, &replayed)
                .map_err(|e| failed(format!("the replay differs from the live run {e}")))?;
            let path = self.snapshot(entry, step);
            fs::write(&path, replayed).map_err(io_error(&path))?;
        }
        Ok(())
    }

    /// Runs `step` on the entry's info and pages steps, collecting a result for each.
    fn run_steps(
        &self,
        entry: &Entry,
        mut step: impl FnMut(Step) -> Result<(), String>,
    ) -> EntryResult {
        let mut result = |s| match step(s) {
            Ok(()) => StepResult {
                passed: true,
                detail: String::new(),
            },
            Err(detail) => StepResult {
                passed: false,
                detail,
            },
        };
        EntryResult {
            name: entry.name.clone(),
            module_id: entry.module_id.clone(),
            info: result(Step::Info),
            pages: result(Step::Pages),
        }
    }

    /// The step's recorded HTTP fixtures (docs/fixtures.md).
    fn fixtures(&self, entry: &Entry, step: Step) -> PathBuf {
        self.dir.join(&entry.name).join(step.command())
    }

    /// The step's snapshot: what `fmd2r module <step>` printed when it was recorded.
    fn snapshot(&self, entry: &Entry, step: Step) -> PathBuf {
        self.dir
            .join(&entry.name)
            .join(format!("{}.json", step.command()))
    }

    /// Runs `fmd2r module <step> <url> --module <id>` in `mode` and returns its stdout, or why
    /// it failed.
    fn run(&self, entry: &Entry, step: Step, mode: Mode) -> Result<String, String> {
        let mut command = Command::new(&self.fmd2r);
        command
            .arg("module")
            .arg(step.command())
            .arg(step.url(entry))
            .arg("--lua-dir")
            .arg(&self.lua_dir)
            .arg("--module")
            .arg(&entry.module_id);
        match mode {
            Mode::Live => {}
            Mode::Record(dir) => {
                command.arg("--record").arg(dir);
            }
            Mode::Replay(dir) => {
                command.arg("--replay").arg(dir).env("PATH", "");
            }
        }
        let output = run_with_timeout(command, TIMEOUT)
            .map_err(|e| format!("cannot run {}: {e}", self.fmd2r.display()))?;
        let Some(output) = output else {
            return Err(format!("timed out after {}s", TIMEOUT.as_secs()));
        };
        if !output.success {
            let stderr = output.stderr.trim();
            return Err(format!("exited with an error: {stderr}"));
        }
        Ok(output.stdout)
    }
}

/// A process's outcome.
struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}

/// Runs `command` and collects its output, or `None` (after killing it) when it outlives
/// `timeout`.
fn run_with_timeout(mut command: Command, timeout: Duration) -> io::Result<Option<Output>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                // A read error leaves what was read so far, which is all a report needs.
                let _ = pipe.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let stdout = reader(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = reader(child.stderr.take().map(|p| Box::new(p) as _));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() > timeout {
            // Fails only when the child has exited meanwhile, which `wait` then reaps.
            let _ = child.kill();
            child.wait()?;
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    Ok(status.map(|status| Output {
        success: status.success(),
        stdout,
        stderr,
    }))
}

/// Passes when `actual` is `expected`, else names the first line that differs (`at line N: ...`).
fn compare(expected: &str, actual: &str) -> Result<(), String> {
    if expected == actual {
        return Ok(());
    }
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    let mut line = 1;
    loop {
        match (expected_lines.next(), actual_lines.next()) {
            (Some(e), Some(a)) if e == a => line += 1,
            (e, a) => {
                return Err(format!(
                    "at line {line}: expected {}, got {}",
                    e.map_or("the end".to_owned(), |e| format!("`{}`", e.trim())),
                    a.map_or("the end".to_owned(), |a| format!("`{}`", a.trim())),
                ));
            }
        }
    }
}

/// Whether a live run found something: `info` a title and chapters with status `no_error`,
/// `pages` a resolved page link (not FMD2's unresolved `W`, baseunits/uDownloadsManager.pas:
/// 845-849) or, for a `DynamicPageLink` module that resolves pages while downloading, a page
/// container link.
pub fn check_output(step: Step, out: &str) -> Result<(), String> {
    let json: Value =
        serde_json::from_str(out).map_err(|e| format!("the output is not JSON: {e}"))?;
    match step {
        Step::Info => {
            let status = json["status"].as_u64();
            if status != Some(0) {
                return Err(format!("OnGetInfo returned status {}", json["status"]));
            }
            if json["info"]["title"].as_str().is_none_or(str::is_empty) {
                return Err("no title".to_owned());
            }
            if json["info"]["chapter_links"]
                .as_array()
                .is_none_or(Vec::is_empty)
            {
                return Err("no chapters".to_owned());
            }
        }
        Step::Pages => {
            let any = |key: &str, resolved: fn(&str) -> bool| {
                json[key]
                    .as_array()
                    .is_some_and(|links| links.iter().filter_map(Value::as_str).any(resolved))
            };
            if !any("page_links", |l| l != "W") && !any("page_container_links", |l| !l.is_empty()) {
                return Err("no resolved page links".to_owned());
            }
        }
    }
    Ok(())
}

/// Drops the recorded response bodies of images in the fixture directory `dir`: the exchange's
/// `body` becomes `null`, as for an empty body (docs/fixtures.md).
fn truncate_media(dir: &Path) -> Result<(), SmokeError> {
    let exchanges = dir.join("exchanges");
    for file in fs::read_dir(&exchanges).map_err(io_error(&exchanges))? {
        let path = file.map_err(io_error(&exchanges))?.path();
        let text = fs::read_to_string(&path).map_err(io_error(&path))?;
        let mut exchange: Value =
            serde_json::from_str(&text).map_err(|source| SmokeError::JsonParse {
                path: path.clone(),
                source,
            })?;
        let response = &mut exchange["response"];
        let is_media = response["headers"].as_array().is_some_and(|headers| {
            headers.iter().any(|h| {
                h[0].as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case("content-type"))
                    && h[1]
                        .as_str()
                        .is_some_and(|v| v.trim().to_ascii_lowercase().starts_with(TRUNCATED_MEDIA))
            })
        });
        let Some(body) = response["body"].as_str().filter(|_| is_media) else {
            continue;
        };
        let body = dir.join(body);
        fs::remove_file(&body).map_err(io_error(&body))?;
        response["body"] = Value::Null;
        let mut json =
            serde_json::to_vec_pretty(&exchange).map_err(|source| SmokeError::JsonWrite {
                what: "an exchange",
                source,
            })?;
        json.push(b'\n');
        fs::write(&path, json).map_err(io_error(&path))?;
    }
    Ok(())
}
