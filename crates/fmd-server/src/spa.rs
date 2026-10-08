//! The web UI (`web/build`), embedded in the binary, with SPA fallback to `index.html`.

use std::borrow::Cow;
use std::collections::HashMap;

use axum::extract::State;
use axum::http::{Method, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

use crate::{ApiError, AppState};

/// Where the static web UI files come from.
pub trait Assets: Send + Sync + 'static {
    /// The file at `path` (relative, no leading slash), if it exists.
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>>;
}

/// `web/build`, embedded at compile time (read from disk in debug builds). Empty when the web UI
/// has not been built.
#[derive(Embed)]
#[folder = "../../web/build"]
#[allow_missing = true]
pub struct EmbeddedAssets;

impl Assets for EmbeddedAssets {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        <Self as Embed>::get(path).map(|f| f.data)
    }
}

/// In-memory files keyed by path; handy for tests and tooling.
impl Assets for HashMap<String, Vec<u8>> {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        HashMap::get(self, path).map(|b| Cow::Owned(b.clone()))
    }
}

const PLACEHOLDER: &str = "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>FMD2r</title></head>\n\
<body><h1>FMD2r</h1><p>The web UI has not been built. Run <code>npm run build</code> in <code>web/</code> \
and rebuild the server. The API is available under <code>/api</code>.</p></body></html>\n";

/// Serves the file at the request path, or `index.html` for any other path so client-side routes
/// work on reload.
/// Paths whose last segment has an extension are files: a missing one is a 404, not the app.
pub(crate) async fn serve(State(state): State<AppState>, method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return ApiError::MethodNotAllowed.into_response();
    }
    let path = uri.path().trim_start_matches('/');
    if !path.is_empty()
        && let Some(data) = state.assets.get(path)
    {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return ([(header::CONTENT_TYPE, mime.as_ref())], data).into_response();
    }
    if path
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
    {
        return ApiError::NotFound.into_response();
    }
    match state.assets.get("index.html") {
        Some(index) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index,
        )
            .into_response(),
        None => Html(PLACEHOLDER).into_response(),
    }
}
