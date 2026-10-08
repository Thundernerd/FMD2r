//! Per-module shared HTTP state.

use std::sync::Arc;

use crate::cookies::CookieJar;
use crate::queue::ConnectionQueue;

/// HTTP state shared by every session of one website module: its connection queue
/// and cookie jar (`ConnectionsQueue` and `CookieManager`, baseunits/WebsiteModules.pas:113,
/// 353-387).
/// Cheap to clone; clones share the state.
#[derive(Clone, Default)]
pub struct ModuleHttp {
    pub(crate) queue: Arc<ConnectionQueue>,
    pub(crate) cookies: Arc<CookieJar>,
}

impl ModuleHttp {
    /// The module's cookie jar.
    pub fn cookies(&self) -> &CookieJar {
        &self.cookies
    }

    /// `MaxConnectionLimit`: concurrent requests allowed for this module, 0 = unlimited
    /// (baseunits/httpsendthread.pas:417-435).
    pub fn max_connections(&self) -> u32 {
        self.queue.max_connections()
    }

    /// `ActiveConnections`: requests of this module holding a connection slot right now
    /// (baseunits/httpsendthread.pas:58, counted at :417-440).
    pub fn active_connections(&self) -> u32 {
        self.queue.active_connections()
    }

    /// Sets `MaxConnectionLimit`; waiting requests re-check the new limit.
    pub fn set_max_connections(&self, max: u32) {
        self.queue.set_max_connections(max);
    }
}
