//! `/api/inbox`: the `events` table as the UI's inbox, which the user marks read and clears.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fmd_core::module_updater;
use fmd_store::{Event, EventId, EventQuery, EventSeverity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::error::ApiQuery;
use crate::{ApiError, AppState, Problem};

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
    /// The event body as plain text, never JSON; it may span several lines.
    pub body: String,
    /// RFC 3339 timestamp.
    pub created_at: String,
    pub read: bool,
    /// The `events` row's kind, e.g. `new_chapters`: what notifications report.
    #[serde(skip)]
    pub(crate) event_kind: String,
}

impl From<Event> for InboxItem {
    fn from(event: Event) -> Self {
        let kind = match event.severity {
            EventSeverity::Info => InboxKind::Info,
            EventSeverity::Warning => InboxKind::Warn,
            EventSeverity::Error => InboxKind::Error,
        };
        let body = readable_body(&event.kind, &event.body);
        Self {
            id: event.id.0.to_string(),
            kind,
            title: event.title,
            body,
            created_at: crate::time::rfc3339_from_unix_ms(event.ts),
            read: event.read,
            event_kind: event.kind,
        }
    }
}

/// A string as-is, the module updater's `{file, names}` / `{file, error}` reports as a sentence
/// or the raw error, anything else as `key: value` lines.
fn readable_body(kind: &str, body: &Value) -> String {
    if kind == module_updater::EVENT_KIND {
        if let Some(Value::String(error)) = body.get("error") {
            return error.clone();
        }
        if let Some(names) = body.get("names") {
            return format!("Unknown Host API names: {}", plain_text(names));
        }
    }
    plain_text(body)
}

/// A JSON value as plain text, without JSON syntax.
fn plain_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(items) => items.iter().map(plain_text).collect::<Vec<_>>().join(", "),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key}: {}", plain_text(value)))
            .collect::<Vec<_>>()
            .join("\n"),
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

/// Mark every inbox item as read.
#[utoipa::path(post, path = "/api/inbox/read", tag = "inbox", operation_id = "markInboxAllRead",
    responses((status = 204, description = "Marked as read")))]
pub(crate) async fn mark_all_read(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.blocking(|db| db.events().mark_all_read()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete one inbox item.
#[utoipa::path(delete, path = "/api/inbox/{id}", tag = "inbox", operation_id = "deleteInboxItem",
    params(("id" = String, Path, description = "Inbox item id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "No such inbox item", body = Problem),
    ))]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = EventId(id.parse().map_err(|_| ApiError::NotFound)?);
    let deleted = state.blocking(move |db| db.events().delete(&[id])).await?;
    if deleted == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Which inbox items `DELETE /api/inbox` deletes.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ClearFilter {
    /// Must be `true`: only read items can be cleared.
    read: Option<bool>,
}

/// Delete every read inbox item.
#[utoipa::path(delete, path = "/api/inbox", tag = "inbox", operation_id = "clearInboxRead",
    params(ClearFilter),
    responses(
        (status = 204, description = "Deleted"),
        (status = 400, description = "`read=true` is missing", body = Problem),
    ))]
pub(crate) async fn clear_read(
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<ClearFilter>,
) -> Result<StatusCode, ApiError> {
    if filter.read != Some(true) {
        return Err(ApiError::BadRequest(
            "only read items can be cleared: pass read=true".into(),
        ));
    }
    state.blocking(|db| db.events().delete_read()).await?;
    Ok(StatusCode::NO_CONTENT)
}
