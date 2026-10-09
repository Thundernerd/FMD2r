//! Domain errors mapped onto RFC 9457 problem responses.

use axum::Json;
use axum::extract::FromRequest;
use axum::extract::FromRequestParts;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

/// An error a handler returns; rendered as `application/problem+json`.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("no such resource")]
    NotFound,
    /// A 404 that says why (e.g. no module handles a URL).
    #[error("{0}")]
    Missing(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("{0}")]
    BadRequest(String),
    /// A well-formed request with a value that fails validation; `field` names the offending
    /// setting (dotted path) so the UI can show the error next to it.
    #[error("{detail}")]
    Invalid {
        field: Option<String>,
        detail: String,
    },
    /// Several values of a well-formed request fail validation; never empty.
    #[error("{}", display_fields(.0))]
    Fields(Vec<FieldProblem>),
    /// The resource is in a state that does not allow the request (e.g. a job already running).
    #[error("{0}")]
    Conflict(String),
    /// A request an extractor could not parse (bad JSON body, bad query string, ...).
    #[error("{1}")]
    Rejected(StatusCode, String),
    /// An upstream site failed to deliver (e.g. a cover).
    #[error("{0}")]
    BadGateway(String),
    /// A service the request needs is not running in this server (e.g. no Lua modules loaded).
    #[error("{0}")]
    Unavailable(String),
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
    /// The setting a validation error (422) is about, as a dotted path such as
    /// `connections.timeout_secs` or `options.server`.
    /// Kept for clients that read one field: the first of `fields`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Every setting a validation error (422) is about, each with why it was rejected.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldProblem>,
}

/// One rejected value of a 422.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FieldProblem {
    /// The setting as a dotted path, like [`Problem::field`].
    pub field: String,
    pub detail: String,
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound | Self::Missing(_) => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Invalid { .. } | Self::Fields(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Rejected(status, _) => *status,
            Self::BadGateway(_) => StatusCode::BAD_GATEWAY,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Store(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        // Server errors go to the log in full; clients only learn that something failed.
        let detail = if let Self::BadGateway(msg) | Self::Unavailable(msg) = &self {
            // Sites failing and services not configured are routine, not server faults: the
            // client may see why.
            tracing::debug!(target: "fmd_server", "{msg}");
            msg.clone()
        } else if status.is_server_error() {
            tracing::error!(target: "fmd_server", "{self}");
            "internal server error".to_owned()
        } else {
            self.to_string()
        };
        let fields = match &self {
            Self::Invalid {
                field: Some(field),
                detail,
            } => vec![FieldProblem {
                field: field.clone(),
                detail: detail.clone(),
            }],
            Self::Fields(fields) => fields.clone(),
            _ => Vec::new(),
        };
        let problem = Problem {
            kind: "about:blank".into(),
            title: status.canonical_reason().unwrap_or("Error").into(),
            status: status.as_u16(),
            detail,
            field: fields.first().map(|f| f.field.clone()),
            fields,
        };
        let mut res = (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            Json(problem),
        )
            .into_response();
        if matches!(self, Self::Unauthorized) {
            res.headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        res
    }
}

impl From<fmd_core::jobs::JobError> for ApiError {
    fn from(err: fmd_core::jobs::JobError) -> Self {
        use fmd_core::jobs::JobError as E;
        match err {
            E::AlreadyRunning | E::NotRunning => Self::Conflict(err.to_string()),
            E::Failed(msg) => Self::Internal(msg),
        }
    }
}

impl From<fmd_core::favorites::CheckError> for ApiError {
    fn from(err: fmd_core::favorites::CheckError) -> Self {
        use fmd_core::favorites::CheckError as E;
        match err {
            E::AlreadyRunning => Self::Conflict(err.to_string()),
            E::Store(e) => Self::Store(e),
            E::NoRuntime | E::Join(_) => Self::Internal(err.to_string()),
        }
    }
}

impl From<fmd_core::lists::ListJobError> for ApiError {
    fn from(err: fmd_core::lists::ListJobError) -> Self {
        use fmd_core::lists::ListJobError as E;
        match err {
            E::UnknownModule(_) => Self::NotFound,
            E::AlreadyRunning(_) | E::NotRunning(_) => Self::Conflict(err.to_string()),
            E::Spawn(_) => Self::Internal(err.to_string()),
        }
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

fn display_fields(fields: &[FieldProblem]) -> String {
    fields
        .iter()
        .map(|f| format!("{}: {}", f.field, f.detail))
        .collect::<Vec<_>>()
        .join("; ")
}
