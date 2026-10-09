//! The network [`Transport`], on reqwest.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::transport::{
    BoxFuture, Proxy, ProxyKind, Transport, TransportError, WireRequest, WireResponse,
};

/// Sends requests with reqwest, configured to stay out of the way of the Synapse
/// semantics layered on top: no automatic redirects, no automatic decompression,
/// no cookie store, HTTP/1.1 with title-cased header names.
///
/// reqwest itself adds `Accept: */*` when the request has no `Accept` header; Synapse
/// would send none. Synapse's `Connection: keep-alive` (baseunits/synapse/httpsend.pas:502-509)
/// is implied by HTTP/1.1 and not sent. FMD2's `Reset` always sets `Accept` (baseunits/httpsendthread.pas:928).
#[derive(Default)]
pub struct ReqwestTransport {
    // One reqwest client per proxy/timeout combination: both are client-level settings.
    clients: Mutex<HashMap<(Option<Proxy>, Duration), reqwest::Client>>,
}

impl ReqwestTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// The client for `request`: shared per proxy/timeout, or a fresh one when the request is
    /// pinned to an address. A pinned client is not reused, since its connections (pooled by
    /// host) must never serve the same host unpinned or pinned elsewhere.
    fn client(&self, request: &WireRequest) -> Result<reqwest::Client, TransportError> {
        let proxy = request.proxy.as_ref().filter(|p| !p.host.is_empty());
        if let (Some(addr), None) = (request.connect_to, proxy) {
            let url = reqwest::Url::parse(&request.url)
                .map_err(|e| TransportError(format!("bad URL: {e}")))?;
            let host = url
                .host_str()
                .ok_or_else(|| TransportError(format!("no host in {url}")))?;
            // reqwest connects to `addr`'s IP on the URL's port.
            return build(proxy, request.timeout, Some((host, addr)));
        }
        let key = (request.proxy.clone(), request.timeout);
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| TransportError("client cache poisoned".into()))?;
        if let Some(client) = clients.get(&key) {
            return Ok(client.clone());
        }
        let client = build(proxy, request.timeout, None)?;
        clients.insert(key, client.clone());
        Ok(client)
    }
}

/// A reqwest client with `proxy` and `timeout`, resolving `pin`'s host to its address.
fn build(
    proxy: Option<&Proxy>,
    timeout: Duration,
    pin: Option<(&str, SocketAddr)>,
) -> Result<reqwest::Client, TransportError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .http1_only()
        .http1_title_case_headers()
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .no_proxy();
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest_proxy(proxy)?);
    }
    if let Some((host, addr)) = pin {
        builder = builder.resolve(host, addr);
    }
    builder.build().map_err(|e| TransportError(e.to_string()))
}

/// Maps `SetProxy` settings onto reqwest. Synapse resolves host names through a
/// SOCKS proxy (`SocksResolver`), hence `socks5h`/`socks4a`.
fn reqwest_proxy(proxy: &Proxy) -> Result<reqwest::Proxy, TransportError> {
    let scheme = match proxy.kind {
        ProxyKind::Http => "http",
        ProxyKind::Socks4 => "socks4a",
        ProxyKind::Socks5 => "socks5h",
    };
    let port = if proxy.port.is_empty() {
        String::new()
    } else {
        format!(":{}", proxy.port)
    };
    let mut url = reqwest::Url::parse(&format!("{scheme}://{}{port}", proxy.host))
        .map_err(|e| TransportError(format!("bad proxy: {e}")))?;
    if !proxy.user.is_empty() {
        url.set_username(&proxy.user)
            .and_then(|()| url.set_password(Some(&proxy.pass)))
            .map_err(|()| TransportError("bad proxy credentials".into()))?;
    }
    reqwest::Proxy::all(url).map_err(|e| TransportError(format!("bad proxy: {e}")))
}

impl Transport for ReqwestTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        let client = self.client(&request);
        Box::pin(async move {
            let client = client?;
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|e| TransportError(e.to_string()))?;
            let mut headers = HeaderMap::new();
            for (name, value) in &request.headers {
                // Synapse writes header lines verbatim; lines hyper cannot encode are dropped.
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    headers.append(name, value);
                }
            }
            let mut builder = client.request(method, &request.url).headers(headers);
            if !request.body.is_empty() {
                builder = builder.body(request.body);
            }
            let response = builder
                .send()
                .await
                .map_err(|e| TransportError(e.to_string()))?;
            let status = response.status();
            let headers = response
                .headers()
                .iter()
                .map(|(n, v)| {
                    (
                        n.as_str().to_string(),
                        String::from_utf8_lossy(v.as_bytes()).into_owned(),
                    )
                })
                .collect();
            let body = response
                .bytes()
                .await
                .map_err(|e| TransportError(e.to_string()))?;
            Ok(WireResponse {
                status: status.as_u16(),
                reason: status.canonical_reason().unwrap_or_default().to_string(),
                headers,
                body: body.to_vec(),
            })
        })
    }
}
