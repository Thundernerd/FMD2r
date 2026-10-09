//! Log lines for the UI: an in-memory ring buffer fed by `tracing`, read via `GET /api/logs`, and
//! optionally persisted to rotated files (`log_files`) so they survive a restart.

use std::collections::VecDeque;
use std::fmt::{self, Write};
use std::io::{self, Read};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::Json;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use utoipa::{IntoParams, ToSchema};

use crate::error::ApiQuery;
use crate::events::{EventBus, ServerEvent};
use crate::log_files::{self, LogRotation, LogWriter};
use crate::{ApiError, AppState};

/// Severity of a log line, most severe first. Serialized in upper case; `?level=` also takes
/// lower case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    #[serde(alias = "error")]
    Error,
    #[serde(alias = "warn")]
    Warn,
    #[serde(alias = "info")]
    Info,
    #[serde(alias = "debug")]
    Debug,
    #[serde(alias = "trace")]
    Trace,
}

/// One log line, as FMD2's log window shows it (mangadownloader/forms/frmLogger.pas).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LogLine {
    /// Monotonic sequence number; pass the last one seen as `GET /api/logs?since=`.
    pub seq: u64,
    /// RFC 3339 timestamp.
    pub time: String,
    pub level: LogLevel,
    pub target: String,
    /// The website module that logged the line (the `module` field `fmd.logger` adds,
    /// baseunits/lua/LuaLogger.pas:15-46).
    pub module: Option<String>,
    pub message: String,
}

/// The newest log lines, fed by `tracing` (install it as a [`Layer`]) and announced on the event
/// bus as `log` events. Cheap to clone.
#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<Ring>>,
    bus: EventBus,
}

struct Ring {
    capacity: usize,
    next_seq: u64,
    lines: VecDeque<LogLine>,
    writer: Option<LogWriter>,
}

impl Ring {
    /// Writes `line` to the log files, when persisted, and buffers it.
    fn record(&mut self, line: LogLine) {
        if let Some(writer) = &mut self.writer {
            // Nowhere to report a failed write: logging it would come straight back here. The
            // write is unbuffered so the lines before a crash are on disk.
            let _ = writer.write(&line);
        }
        self.push(line);
    }

    fn push(&mut self, line: LogLine) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}

impl LogBuffer {
    /// Keeps the newest `capacity` lines and publishes each new one on `bus`.
    pub fn new(capacity: usize, bus: EventBus) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Ring {
                capacity: capacity.max(1),
                next_seq: 1,
                lines: VecDeque::new(),
                writer: None,
            })),
            bus,
        }
    }

    /// Writes every line to the files in `dir` from now on, rotated as `rotation` says, after
    /// loading the newest lines already there (from before a restart) into the buffer. Sequence
    /// numbers continue after the persisted ones, so `since` pages forward across the restart;
    /// lines buffered before this call are renumbered after them and written too.
    pub fn persist(&self, dir: &Path, rotation: LogRotation) -> io::Result<()> {
        let writer = LogWriter::open(dir, rotation)?;
        let capacity = self.lock()?.capacity;
        // Read before touching the ring, so a failed read leaves it as it was.
        let (tail, mut last_seq) = log_files::read_tail(dir, capacity)?;
        if writer.repaired {
            // The partial line was the newest; its sequence number may have been seen.
            last_seq += 1;
        }
        let mut ring = self.lock()?;
        let pending: Vec<LogLine> = ring.lines.drain(..).collect();
        for line in tail {
            ring.push(line);
        }
        ring.next_seq = last_seq + 1;
        ring.writer = Some(writer);
        for mut line in pending {
            line.seq = ring.next_seq;
            ring.next_seq += 1;
            ring.record(line);
        }
        Ok(())
    }

    /// The persisted log files concatenated, oldest line first (JSON lines); the buffered lines
    /// in the same format when the log isn't persisted.
    pub fn export(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let files = {
            let ring = self.lock()?;
            let Some(writer) = &ring.writer else {
                for line in &ring.lines {
                    serde_json::to_writer(&mut out, line).map_err(io::Error::other)?;
                    out.push(b'\n');
                }
                return Ok(out);
            };
            writer.snapshot()?
        };
        // Read outside the lock, up to each file's length at the snapshot, so logging goes on.
        for (file, len) in files {
            file.take(len).read_to_end(&mut out)?;
        }
        Ok(out)
    }

    fn lock(&self) -> io::Result<MutexGuard<'_, Ring>> {
        self.inner
            .lock()
            .map_err(|_| io::Error::other("log buffer lock poisoned"))
    }

    /// The bus new lines are published on.
    pub fn events(&self) -> &EventBus {
        &self.bus
    }

    /// Buffered lines with a sequence number above `since` (all of them for `None`), oldest first.
    pub fn since(&self, since: Option<u64>) -> Vec<LogLine> {
        self.query(&LogFilter {
            since,
            ..LogFilter::default()
        })
    }

    /// Buffered lines matching `filter`, oldest first. With `since`, a limit keeps the oldest
    /// lines after it, so a client can page forward without gaps; otherwise the newest.
    pub fn query(&self, filter: &LogFilter) -> Vec<LogLine> {
        let Ok(ring) = self.inner.lock() else {
            return Vec::new();
        };
        let limit = filter.limit.unwrap_or(usize::MAX);
        let matching = ring.lines.iter().filter(|l| filter.matches(l));
        if filter.since.is_some() {
            return matching.take(limit).cloned().collect();
        }
        let mut lines: Vec<LogLine> = matching.rev().take(limit).cloned().collect();
        lines.reverse();
        lines
    }

    fn push(&self, level: LogLevel, target: &str, module: Option<String>, message: String) {
        let line = {
            let Ok(mut ring) = self.inner.lock() else {
                return;
            };
            let line = LogLine {
                seq: ring.next_seq,
                time: crate::time::now_rfc3339(),
                level,
                target: target.to_owned(),
                module,
                message,
            };
            ring.next_seq += 1;
            ring.record(line.clone());
            line
        };
        // Debug and trace lines stay in the buffer: streaming them could crowd task and inbox
        // events out of the bus.
        if level <= LogLevel::Info {
            self.bus.publish(ServerEvent::Log(line));
        }
    }
}

impl<S: Subscriber> Layer<S> for LogBuffer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = match *meta.level() {
            Level::ERROR => LogLevel::Error,
            Level::WARN => LogLevel::Warn,
            Level::INFO => LogLevel::Info,
            Level::DEBUG => LogLevel::Debug,
            Level::TRACE => LogLevel::Trace,
        };
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let module = visitor.module.take();
        self.push(level, meta.target(), module, visitor.finish());
    }
}

/// Renders an event as its message followed by ` key=value` for every other field except
/// `module`, which is kept apart.
#[derive(Default)]
struct MessageVisitor {
    message: String,
    module: Option<String>,
    fields: String,
}

impl MessageVisitor {
    fn finish(self) -> String {
        self.message + &self.fields
    }
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else if field.name() == "module" {
            self.module = Some(value.to_owned());
        } else {
            let _ = write!(self.fields, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

/// Which buffered lines [`LogBuffer::query`] returns.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogFilter {
    /// Only lines at this level or more severe.
    #[param(value_type = Option<String>, example = "warn")]
    pub level: Option<LogLevel>,
    /// Only lines logged by this website module.
    pub module: Option<String>,
    /// Only lines with a sequence number above this one.
    pub since: Option<u64>,
    /// At most this many lines: the oldest after `since` when given, else the newest.
    pub limit: Option<usize>,
}

impl LogFilter {
    fn matches(&self, line: &LogLine) -> bool {
        self.since.is_none_or(|since| line.seq > since)
            && self.level.is_none_or(|level| line.level <= level)
            && self
                .module
                .as_deref()
                .is_none_or(|module| line.module.as_deref() == Some(module))
    }
}

/// The tail of the in-memory log, oldest first.
#[utoipa::path(get, path = "/api/logs", tag = "system", operation_id = "listLogs",
    params(LogFilter), responses((status = 200, body = Vec<LogLine>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<LogFilter>,
) -> Json<Vec<LogLine>> {
    Json(state.logs.query(&filter))
}

/// Download the persisted log files (or, when the log isn't persisted, the buffered lines) as
/// one JSON-lines attachment, oldest line first.
#[utoipa::path(get, path = "/api/logs/download", tag = "system", operation_id = "downloadLogs",
    responses(
        (status = 200, description = "One JSON log line per line", content_type = "application/x-ndjson"),
    ))]
pub(crate) async fn download(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let logs = state.logs.clone();
    let body = tokio::task::spawn_blocking(move || logs.export())
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"fmd2r-logs.jsonl\"",
            ),
        ],
        body,
    ))
}
