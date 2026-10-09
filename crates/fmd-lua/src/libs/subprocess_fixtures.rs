//! Subprocess fixtures: a [`Spawner`] that records every process `fmd.subprocess` runs into a
//! fixture directory, next to the HTTP fixtures, and one that answers from them offline. A
//! module that runs node (lua/utils/nodejs.lua) fetches pages from inside that process, past
//! the recorded HTTP; replaying the process's output makes it run offline too. The format is
//! documented in `docs/fixtures.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::subprocess::{Command, Output, Spawner};

/// The version of the format `subprocess.json` declares.
pub const SUBPROCESS_FORMAT: u32 = 1;

/// The file the calls are recorded in, in the fixture directory.
pub const SUBPROCESS_FILE: &str = "subprocess.json";

/// Errors reading or writing a subprocess fixture file.
#[derive(Debug, thiserror::Error)]
pub enum SubprocessFixtureError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path}: subprocess fixture format {found}, expected {SUBPROCESS_FORMAT}")]
    Format { path: PathBuf, found: u32 },
}

/// `subprocess.json`: every process started, in order.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Calls {
    format: u32,
    calls: Vec<Call>,
}

/// One process: what was started and what it produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Call {
    program: String,
    args: Vec<String>,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Output(RecordedOutput),
    /// The process could not be started, with this message.
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordedOutput {
    stdout: Bytes,
    stderr: Bytes,
    status: i32,
}

/// Output as text when it is UTF-8, so fixtures stay readable, else as a byte array.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Bytes {
    Text(String),
    Raw(Vec<u8>),
}

impl From<&[u8]> for Bytes {
    fn from(bytes: &[u8]) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => Bytes::Text(text.to_owned()),
            Err(_) => Bytes::Raw(bytes.to_vec()),
        }
    }
}

impl Bytes {
    fn to_vec(&self) -> Vec<u8> {
        match self {
            Bytes::Text(text) => text.as_bytes().to_vec(),
            Bytes::Raw(bytes) => bytes.clone(),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `program args...`, for messages.
fn describe(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs processes through another spawner and records each call in `subprocess.json`.
pub struct RecordingSpawner {
    inner: Arc<dyn Spawner + Send + Sync>,
    path: PathBuf,
    calls: Mutex<Vec<Call>>,
}

impl RecordingSpawner {
    /// Records the processes `inner` runs into `dir`, replacing the `subprocess.json` a
    /// previous recording left there. The file is only written once a process runs.
    pub fn new(
        dir: impl AsRef<Path>,
        inner: Arc<dyn Spawner + Send + Sync>,
    ) -> Result<Self, SubprocessFixtureError> {
        let path = dir.as_ref().join(SUBPROCESS_FILE);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|source| SubprocessFixtureError::Io {
                path: path.clone(),
                source,
            })?;
        }
        Ok(Self {
            inner,
            path,
            calls: Mutex::default(),
        })
    }

    fn write(&self, calls: &[Call]) -> std::io::Result<()> {
        let file = Calls {
            format: SUBPROCESS_FORMAT,
            calls: calls.to_vec(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(std::io::Error::other)? + "\n";
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, json)
    }
}

impl Spawner for RecordingSpawner {
    /// Runs `command` through the inner spawner and records it. Failing to write the recording
    /// fails the call, so an incomplete recording doesn't go unnoticed.
    fn run(&self, command: &Command) -> std::io::Result<Output> {
        let result = self.inner.run(command);
        let outcome = match &result {
            Ok(output) => Outcome::Output(RecordedOutput {
                stdout: output.stdout.as_slice().into(),
                stderr: output.stderr.as_slice().into(),
                status: output.status,
            }),
            Err(e) => Outcome::Error(e.to_string()),
        };
        let mut calls = lock(&self.calls);
        calls.push(Call {
            program: command.program.clone(),
            args: command.args.clone(),
            outcome,
        });
        self.write(&calls)?;
        result
    }
}

/// Answers processes from a `subprocess.json` without starting any.
///
/// A call is answered by the recorded calls with the same program and arguments, in recording
/// order; once they're used up, the last one answers every further repeat. A call with no
/// recording fails to start, like a missing program, and is listed by
/// [`misses`](ReplaySpawner::misses).
pub struct ReplaySpawner {
    calls: Vec<Call>,
    served: Mutex<Vec<usize>>,
    misses: Mutex<Vec<String>>,
}

impl ReplaySpawner {
    /// Loads the calls recorded in `dir`; a directory without `subprocess.json` has none.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, SubprocessFixtureError> {
        let path = dir.as_ref().join(SUBPROCESS_FILE);
        let calls = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let file: Calls =
                    serde_json::from_str(&text).map_err(|source| SubprocessFixtureError::Json {
                        path: path.clone(),
                        source,
                    })?;
                if file.format != SUBPROCESS_FORMAT {
                    return Err(SubprocessFixtureError::Format {
                        path,
                        found: file.format,
                    });
                }
                file.calls
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(source) => return Err(SubprocessFixtureError::Io { path, source }),
        };
        Ok(Self {
            served: Mutex::new(vec![0; calls.len()]),
            calls,
            misses: Mutex::default(),
        })
    }

    /// Every call that had no recording, as `program args...`, in the order they came.
    pub fn misses(&self) -> Vec<String> {
        lock(&self.misses).clone()
    }
}

impl Spawner for ReplaySpawner {
    fn run(&self, command: &Command) -> std::io::Result<Output> {
        let matching: Vec<usize> = (0..self.calls.len())
            .filter(|&i| self.calls[i].program == command.program)
            .filter(|&i| self.calls[i].args == command.args)
            .collect();
        let Some(&last) = matching.last() else {
            let miss = describe(&command.program, &command.args);
            lock(&self.misses).push(miss.clone());
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no recorded call for {miss}"),
            ));
        };
        let i = {
            let mut served = lock(&self.served);
            let next = matching
                .iter()
                .copied()
                .find(|&i| served[i] == 0)
                .unwrap_or(last);
            served[next] += 1;
            next
        };
        match &self.calls[i].outcome {
            Outcome::Output(output) => Ok(Output {
                stdout: output.stdout.to_vec(),
                stderr: output.stderr.to_vec(),
                status: output.status,
            }),
            Outcome::Error(message) => Err(std::io::Error::other(message.clone())),
        }
    }
}
