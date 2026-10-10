//! Blocking HTTP façade with Synapse semantics on top of reqwest.
//!
//! [`HttpSession`] requests block the calling thread on the client's tokio runtime. They are
//! meant for Lua worker threads and return [`HttpError::InsideRuntime`] when called from
//! inside a runtime.

mod client;
mod cookies;
mod decode;
mod fixtures;
mod httpdate;
mod module;
mod queue;
mod reqwest_transport;
mod session;
mod strings;
mod terminate;
mod transport;
mod url;

pub use client::HttpClient;
pub use cookies::{Cookie, CookieJar};
pub use fixtures::{
    FIXTURE_FORMAT, FixtureError, RecordingTransport, ReplayOptions, ReplayTransport,
};
pub use module::ModuleHttp;
pub use reqwest_transport::ReqwestTransport;
pub use session::{HttpSession, SessionHook, USER_AGENT_DEFAULT, USER_AGENT_SYNAPSE};
pub use strings::NameValueList;
pub use terminate::TerminateToken;
pub use transport::{
    BoxFuture, ConnectTo, Proxy, ProxyKind, Transport, TransportError, WireRequest, WireResponse,
};
pub use url::split_url_bytes;

/// Errors from the `fmd-http` API itself; like FMD2, network failures just return `false`.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("blocking HTTP call made from inside a tokio runtime")]
    InsideRuntime,
    #[error("cannot start the HTTP runtime: {0}")]
    Runtime(#[from] std::io::Error),
    #[error("invalid cookie jar data: {0}")]
    CookieJar(#[from] serde_json::Error),
}
