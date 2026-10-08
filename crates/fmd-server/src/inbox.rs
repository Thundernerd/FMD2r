//! `GET /api/inbox` and `POST /api/inbox/{id}/read`: the `events` table as the UI's inbox.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_store::{Event, EventId, EventQuery, EventSeverity};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{ApiError, AppState, Problem};

/// How many inbox items `GET /api/inbox` returns at most.
const INBOX_LIMIT: u32 = 200;

/// How urgent an inbox item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum InboxKind {
    Info,
    Warn,
    Error,
}

/// One inbox item (a row of the `events` table).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct InboxItem {
    pub id: String,
    pub kind: InboxKind,
    pub title: String,
    /// The event body: a string as-is, anything else as JSON text.
    pub body: String,
    /// RFC 3339 timestamp.
    pub created_at: String,
    pub read: bool,
}

impl From<Event> for InboxItem {
    fn from(event: Event) -> Self {
        let kind = match event.severity {
            EventSeverity::Info => InboxKind::Info,
            EventSeverity::Warning => InboxKind::Warn,
            EventSeverity::Error => InboxKind::Error,
        };
        let body = match event.body {
            serde_json::Value::String(s) => s,
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        };
        Self {
            id: event.id.0.to_string(),
            kind,
            title: event.title,
            body,
            created_at: crate::time::rfc3339_from_unix_ms(event.ts),
            read: event.read,
        }
    }
}

/// Inbox items, newest first.
#[utoipa::path(get, path = "/api/inbox", tag = "inbox", operation_id = "listInbox",
    responses((status = 200, body = Vec<InboxItem>)))]
pub(crate) async fn list(State(state): State<AppState>) -> Result<Json<Vec<InboxItem>>, ApiError> {
    let events = state
        .blocking(|db| {
            db.events().list(&EventQuery {
                unread_only: false,
                limit: Some(INBOX_LIMIT),
            })
        })
        .await?;
    Ok(Json(events.into_iter().map(InboxItem::from).collect()))
}

/// Mark one inbox item as read.
#[utoipa::path(post, path = "/api/inbox/{id}/read", tag = "inbox", operation_id = "markInboxRead",
    params(("id" = String, Path, description = "Inbox item id")),
    responses(
        (status = 204, description = "Marked as read"),
        (status = 404, description = "No such inbox item", body = Problem),
    ))]
pub(crate) async fn mark_read(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = EventId(id.parse().map_err(|_| ApiError::NotFound)?);
    state
        .blocking(move |db| {
            if db.events().get(id)?.is_none() {
                return Err(ApiError::NotFound);
            }
            Ok(db.events().mark_read(&[id])?)
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
