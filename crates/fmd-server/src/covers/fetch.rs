//! Fetching a cover upstream through the owning module's HTTP session.

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

use fmd_http::HttpError;
use thiserror::Error;
use url::Url;

use super::{CoverModules, CoverSession};

/// Redirects followed before giving up, like `THTTPSendThread.MaxRedirect`
/// (baseunits/httpsendthread.pas:516).
const MAX_REDIRECTS: u32 = 5;

/// Validators of a cached copy, for a conditional request.
#[derive(Debug, Clone, Default)]
pub(crate) struct Validators {
    pub(crate) etag: Option<String>,
    pub(crate) last_modified: Option<String>,
}

pub(crate) enum Fetched {
    Body {
        body: Vec<u8>,
        content_type: String,
        validators: Validators,
    },
    /// Upstream confirmed the cached copy (304).
    NotModified,
}

#[derive(Debug, Error)]
pub(crate) enum FetchError {
    #[error("unknown module {0:?}")]
    UnknownModule(String),
    /// The URL may not be fetched (SSRF guard).
    #[error("{0}")]
    Forbidden(String),
    #[error("upstream: {0}")]
    Upstream(String),
    #[error(transparent)]
    Http(#[from] HttpError),
}

/// GETs `url` with a session prepared for `module` like FMD2 fetches a cover: the module's
/// cookies, user agent and connection queue (`TModuleContainer.PrepareHTTP`,
/// baseunits/WebsiteModules.pas:353-387; `TGetMangaInfosThread`, baseunits/uGetMangaInfosThread.pas:143-147),
/// plus `Referer: <RootURL>/` since sites often refuse hotlinked images.
///
/// Blocks: run it on a thread outside the tokio runtime (see `fmd_http`'s threading contract).
pub(crate) fn fetch(
    modules: &dyn CoverModules,
    module: &str,
    url: &Url,
    cached: Option<&Validators>,
) -> Result<Fetched, FetchError> {
    let CoverSession {
        root_url,
        mut session,
    } = modules
        .cover_session(module)
        .ok_or_else(|| FetchError::UnknownModule(module.to_owned()))?;
    let referer = format!("{}/", root_url.trim_end_matches('/'));
    let root = Url::parse(&root_url).ok();
    // Redirects are followed here, not by the session, so every hop passes the SSRF guard.
    session.set_follow_redirection(false);
    let mut url = url.clone();
    for _ in 0..=MAX_REDIRECTS {
        guard(&url, root.as_ref())?;
        session.reset();
        let headers = session.headers_mut();
        headers.set_value("Referer", &referer);
        headers.set_value(
            "Accept",
            "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
        );
        if let Some(cached) = cached {
            if let Some(etag) = &cached.etag {
                headers.set_value("If-None-Match", etag);
            }
            if let Some(date) = &cached.last_modified {
                headers.set_value("If-Modified-Since", date);
            }
        }
        session.get(url.as_str())?;
        let code = session.result_code();
        match code {
            301 | 302 | 303 | 307 | 308 => {
                let location = session.headers().value("Location").trim().to_owned();
                url = url
                    .join(&location)
                    .map_err(|e| FetchError::Upstream(format!("bad redirect {location:?}: {e}")))?;
            }
            304 if cached.is_some() => return Ok(Fetched::NotModified),
            200..=299 if !session.document().is_empty() => {
                let header = |name| {
                    Some(session.headers().value(name).trim().to_owned()).filter(|v| !v.is_empty())
                };
                return Ok(Fetched::Body {
                    validators: Validators {
                        etag: header("ETag"),
                        last_modified: header("Last-Modified"),
                    },
                    content_type: session.mime_type().to_owned(),
                    body: std::mem::take(session.document_mut()),
                });
            }
            _ => return Err(FetchError::Upstream(format!("{url} answered {code}"))),
        }
    }
    Err(FetchError::Upstream(format!(
        "too many redirects for {url}"
    )))
}

/// The SSRF guard: only http(s), and no private-network target unless it is the module's own host
/// (a module for a site on the LAN may fetch its covers there).
fn guard(url: &Url, root: Option<&Url>) -> Result<(), FetchError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::Forbidden(format!("not an http(s) URL: {url}")));
    }
    let Some(host) = url.host() else {
        return Err(FetchError::Forbidden(format!("no host in {url}")));
    };
    if root.is_some_and(|root| {
        root.host() == Some(host.clone())
            && root.port_or_known_default() == url.port_or_known_default()
    }) {
        return Ok(());
    }
    let addrs: Vec<IpAddr> = match host {
        url::Host::Ipv4(ip) => vec![IpAddr::V4(ip)],
        url::Host::Ipv6(ip) => vec![IpAddr::V6(ip)],
        url::Host::Domain(domain) => (domain, url.port_or_known_default().unwrap_or(80))
            .to_socket_addrs()
            .map_err(|e| FetchError::Upstream(format!("cannot resolve {domain}: {e}")))?
            .map(|a| a.ip())
            .collect(),
    };
    if addrs.iter().any(|ip| is_private(*ip)) {
        return Err(FetchError::Forbidden(format!(
            "{url} points into a private network"
        )));
    }
    Ok(())
}

/// Loopback, private, link-local, shared (CGNAT), unspecified, broadcast or multicast addresses.
fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_private_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => is_private_v4(v4),
            None => {
                let first = ip.segments()[0];
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    // Unique local fc00::/7 and link-local fe80::/10.
                    || first & 0xfe00 == 0xfc00
                    || first & 0xffc0 == 0xfe80
            }
        },
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0
        // Shared address space 100.64.0.0/10.
        || (a == 100 && (64..128).contains(&b))
}
