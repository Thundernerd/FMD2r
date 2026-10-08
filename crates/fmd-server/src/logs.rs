//! Log lines for the UI: an in-memory ring buffer fed by `tracing`, read via `GET /api/logs`.

use std::collections::VecDeque;
use std::fmt::{self, Write};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::error::ApiQuery;
use crate::events::{EventBus, ServerEvent};

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
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
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
}

impl LogBuffer {
    /// Keeps the newest `capacity` lines and publishes each new one on `bus`.
    pub fn new(capacity: usize, bus: EventBus) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Ring {
                capacity: capacity.max(1),
                next_seq: 1,
                lines: VecDeque::new(),
            })),
            bus,
        }
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

    /// The newest buffered lines matching `filter`, oldest first.
    pub fn query(&self, filter: &LogFilter) -> Vec<LogLine> {
        let Ok(ring) = self.inner.lock() else {
            return Vec::new();
        };
        let mut lines: Vec<LogLine> = ring
            .lines
            .iter()
            .rev()
            .filter(|l| filter.matches(l))
            .take(filter.limit.unwrap_or(usize::MAX))
            .cloned()
            .collect();
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
            if ring.lines.len() == ring.capacity {
                ring.lines.pop_front();
            }
            ring.lines.push_back(line.clone());
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
    /// At most this many lines: the newest that match.
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
