//! Where the MangaBaka dump is downloaded from.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use fmd_http::TerminateToken;

use super::MetadataError;
use crate::settings::{ProxyType, SettingsService};

/// Reading has no overall limit: the dump is a few hundred megabytes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Sent to MangaBaka and MangaDex.
pub const USER_AGENT: &str = concat!(
    "FMD2r/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Thundernerd/FMD2r)"
);

pub struct Download {
    /// Still compressed.
    pub reader: Box<dyn Read + Send>,
    pub length: Option<u64>,
}

pub trait DumpSource: Send + Sync + 'static {
    /// Blocks until the response headers arrive.
    fn open(&self, url: &str, terminate: &TerminateToken) -> Result<Download, MetadataError>;
}

/// Downloads over HTTP through the global proxy.
pub struct HttpDumpSource {
    settings: Arc<SettingsService>,
}

impl HttpDumpSource {
    pub fn new(settings: Arc<SettingsService>) -> Self {
        Self { settings }
    }

    fn client(&self) -> Result<reqwest::blocking::Client, MetadataError> {
        let connections = self.settings.get().connections.clone();
        let mut builder = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(None);
        let proxy = &connections.proxy;
        if proxy.enabled && !proxy.host.is_empty() {
            let scheme = match proxy.kind {
                ProxyType::Http => "http",
                ProxyType::Socks4 => "socks4",
                ProxyType::Socks5 => "socks5h",
            };
            let url = match proxy.port {
                Some(port) => format!("{scheme}://{}:{port}", proxy.host),
                None => format!("{scheme}://{}", proxy.host),
            };
            let mut p = reqwest::Proxy::all(url).map_err(|e| MetadataError::Http(e.to_string()))?;
            if !proxy.username.is_empty() {
                p = p.basic_auth(&proxy.username, &proxy.password);
            }
            builder = builder.proxy(p);
        }
        builder
            .build()
            .map_err(|e| MetadataError::Http(e.to_string()))
    }
}

impl DumpSource for HttpDumpSource {
    fn open(&self, url: &str, terminate: &TerminateToken) -> Result<Download, MetadataError> {
        if terminate.is_terminated() {
            return Err(MetadataError::Cancelled);
        }
        let response = self
            .client()?
            .get(url)
            .send()
            .map_err(|e| MetadataError::Http(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(MetadataError::Download {
                url: url.to_owned(),
                status: status.as_u16(),
            });
        }
        let length = response.content_length();
        Ok(Download {
            reader: Box::new(response),
            length,
        })
    }
}
