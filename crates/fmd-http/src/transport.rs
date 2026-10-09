//! The wire-level seam: one HTTP exchange, no redirects, retries or decoding.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRequest {
    pub method: String,
    pub url: String,
    /// Header name/value pairs in send order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Socket timeout (connect and per read), like Synapse's `Sock.SetTimeout`.
    pub timeout: Duration,
    pub proxy: Option<Proxy>,
    /// Unused with a proxy.
    pub connect_to: Option<ConnectTo>,
}

/// A host pinned to an address; the host still names the server for TLS and the `Host` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectTo {
    pub host: String,
    pub addr: SocketAddr,
}

impl ConnectTo {
    pub fn applies_to(&self, url: &str) -> bool {
        reqwest::Url::parse(url)
            .ok()
            .and_then(|url| {
                url.host_str()
                    .map(|host| host.eq_ignore_ascii_case(&self.host))
            })
            .unwrap_or(false)
    }
}

/// The body is still content-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireResponse {
    pub status: u16,
    /// Reason phrase of the status line; may be empty.
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Any transport failure; FMD2 treats them alike: `HTTPMethod` returns false with
/// `ResultCode` 500 (baseunits/synapse/httpsend.pas:436, 553-558).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("transport error: {0}")]
pub struct TransportError(pub String);

/// Sends one HTTP exchange. The future runs on the client's runtime and is dropped
/// mid-flight when the session is terminated.
pub trait Transport: Send + Sync + 'static {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>>;
}

/// Proxy protocol, as accepted by `SetProxy` (baseunits/httpsendthread.pas:838-875).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProxyKind {
    Http,
    Socks4,
    Socks5,
}

/// Empty credentials mean none.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Proxy {
    pub kind: ProxyKind,
    pub host: String,
    pub port: String,
    pub user: String,
    pub pass: String,
}
