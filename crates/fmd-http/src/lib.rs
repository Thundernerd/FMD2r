//! Blocking HTTP façade with Synapse semantics on top of reqwest (gzip/br/zstd, socks/http proxy, cookie jar per module, per-module connection queue).
//!
//! # Threading contract
//!
//! [`HttpClient`] owns (or borrows) a multi-threaded tokio runtime. An [`HttpSession`]'s
//! request methods block the calling thread with [`tokio::runtime::Handle::block_on`] while
//! the exchange runs on that runtime. They are meant for the dedicated Lua worker threads
//! (OS threads outside any runtime) and refuse to run on a thread that is inside a tokio
//! runtime context: they return [`HttpError::InsideRuntime`] instead of blocking it.

mod client;
mod cookies;
mod decode;
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
pub use module::ModuleHttp;
pub use reqwest_transport::ReqwestTransport;
pub use session::{HttpSession, USER_AGENT_DEFAULT, USER_AGENT_SYNAPSE};
pub use strings::NameValueList;
pub use terminate::TerminateToken;
pub use transport::{
    BoxFuture, Proxy, ProxyKind, Transport, TransportError, WireRequest, WireResponse,
};
pub use url::split_url;

/// Errors from the `fmd-http` API itself. Network failures are not errors: like FMD2,
/// they make a request return `false`.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("blocking HTTP call made from inside a tokio runtime")]
    InsideRuntime,
    #[error("cannot start the HTTP runtime: {0}")]
    Runtime(#[from] std::io::Error),
    #[error("invalid cookie jar data: {0}")]
    CookieJar(#[from] serde_json::Error),
}
