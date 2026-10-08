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

/// Severity of a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
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

    /// Buffered lines with a sequence number above `since` (all of them for `None`), oldest first.
    pub fn since(&self, since: Option<u64>) -> Vec<LogLine> {
        let Ok(ring) = self.inner.lock() else {
            return Vec::new();
        };
        let since = since.unwrap_or(0);
        ring.lines
            .iter()
            .filter(|l| l.seq > since)
            .cloned()
            .collect()
    }

    fn push(&self, level: LogLevel, target: &str, message: String) {
        let line = {
            let Ok(mut ring) = self.inner.lock() else {
                return;
            };
            let line = LogLine {
                seq: ring.next_seq,
                time: crate::time::now_rfc3339(),
                level,
                target: target.to_owned(),
                message,
            };
            ring.next_seq += 1;
            if ring.lines.len() == ring.capacity {
                ring.lines.pop_front();
            }
            ring.lines.push_back(line.clone());
            line
        };
        self.bus.publish(ServerEvent::Log(line));
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
        let mut message = MessageVisitor::default();
        event.record(&mut message);
        self.push(level, meta.target(), message.finish());
    }
}

/// Renders an event as its message followed by ` key=value` for every other field.
#[derive(Default)]
struct MessageVisitor {
    message: String,
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

#[derive(Deserialize, IntoParams)]
pub(crate) struct LogsQuery {
    /// Only lines with a sequence number above this one.
    since: Option<u64>,
}

/// The tail of the in-memory log.
#[utoipa::path(get, path = "/api/logs", tag = "system", operation_id = "listLogs",
    params(LogsQuery), responses((status = 200, body = Vec<LogLine>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<LogsQuery>,
) -> Json<Vec<LogLine>> {
    Json(state.logs.since(query.since))
}
