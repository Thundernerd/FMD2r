//! Domain errors mapped onto RFC 9457 problem responses.

use axum::Json;
use axum::extract::FromRequest;
use axum::extract::FromRequestParts;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

/// An error a handler returns; rendered as `application/problem+json`.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("no such resource")]
    NotFound,
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    BadRequest(String),
    /// A request an extractor could not parse (bad JSON body, bad query string, ...).
    #[error("{1}")]
    Rejected(StatusCode, String),
    #[error(transparent)]
    Store(#[from] fmd_store::StoreError),
    #[error("internal error: {0}")]
    Internal(String),
}

/// RFC 9457 problem details body.
#[derive(Serialize, ToSchema)]
pub struct Problem {
    /// Always `about:blank`: the status code says it all.
    #[serde(rename = "type")]
    pub kind: String,
    /// The status code's reason phrase.
    pub title: String,
    pub status: u16,
    pub detail: String,
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Rejected(status, _) => *status,
            Self::Store(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(target: "fmd_server", "{self}");
        }
        let problem = Problem {
            kind: "about:blank".into(),
            title: status.canonical_reason().unwrap_or("Error").into(),
            status: status.as_u16(),
            detail: self.to_string(),
        };
        (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            Json(problem),
        )
            .into_response()
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::Rejected(rejection.status(), rejection.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
        Self::Rejected(rejection.status(), rejection.body_text())
    }
}

/// `axum::Json` whose rejection is a problem response.
#[derive(FromRequest)]
#[from_request(via(Json), rejection(ApiError))]
pub(crate) struct ApiJson<T>(pub T);

/// `axum::extract::Query` whose rejection is a problem response.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub(crate) struct ApiQuery<T>(pub T);
