//! The shared client: runtime, transport and process-wide defaults.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::runtime::{Handle, Runtime};

use crate::HttpError;
use crate::module::ModuleHttp;
use crate::reqwest_transport::ReqwestTransport;
use crate::session::HttpSession;
use crate::session::USER_AGENT_DEFAULT;
use crate::transport::{Proxy, Transport};

/// Shared HTTP client. Cheap to clone; every clone shares the runtime, transport and defaults.
#[derive(Clone)]
pub struct HttpClient {
    pub(crate) inner: Arc<ClientInner>,
}

pub(crate) struct ClientInner {
    // Taken on drop to shut down without blocking.
    runtime: Option<Runtime>,
    pub(crate) handle: Handle,
    pub(crate) transport: Arc<dyn Transport>,
    modules: Mutex<HashMap<String, ModuleHttp>>,
    defaults: Mutex<Defaults>,
    generation: AtomicU64,
}

impl Drop for ClientInner {
    /// The last clone may be dropped on an async thread, where a blocking runtime
    /// shutdown would panic.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// A setting stamped with when it was last changed, so the newer of a session's own
/// value and the client default wins. This reproduces FMD2's `Set…AndApply`, which
/// pushes a changed default into every live session
/// (baseunits/httpsendthread.pas:347-392).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setting<T> {
    pub(crate) value: T,
    pub(crate) generation: u64,
}

impl<T: Clone> Setting<T> {
    /// The newer of `self` (a session's value) and `default`.
    pub(crate) fn effective(&self, default: &Setting<T>) -> T {
        if self.generation >= default.generation {
            self.value.clone()
        } else {
            default.value.clone()
        }
    }
}

/// Process-wide defaults (baseunits/httpsendthread.pas:163-171).
#[derive(Debug, Clone)]
pub(crate) struct Defaults {
    pub(crate) user_agent: String,
    pub(crate) retry_count: Setting<i32>,
    pub(crate) timeout_ms: Setting<u32>,
    pub(crate) proxy: Setting<Option<Proxy>>,
}

impl HttpClient {
    /// A client that sends over the network with [`ReqwestTransport`] on its own runtime.
    pub fn new() -> Result<Self, HttpError> {
        Self::with_transport(Arc::new(ReqwestTransport::new()))
    }

    /// A client that sends through `transport` on its own runtime.
    pub fn with_transport(transport: Arc<dyn Transport>) -> Result<Self, HttpError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("fmd-http")
            .enable_all()
            .build()?;
        Ok(Self {
            inner: Arc::new(ClientInner {
                handle: runtime.handle().clone(),
                runtime: Some(runtime),
                transport,
                modules: Mutex::default(),
                defaults: Mutex::new(Defaults {
                    user_agent: USER_AGENT_DEFAULT.into(),
                    retry_count: Setting {
                        value: 0,
                        generation: 0,
                    },
                    timeout_ms: Setting {
                        value: 15000,
                        generation: 0,
                    },
                    proxy: Setting {
                        value: None,
                        generation: 0,
                    },
                }),
                generation: AtomicU64::new(1),
            }),
        })
    }

    /// A new session not bound to any module.
    pub fn session(&self) -> HttpSession {
        HttpSession::new(self.clone(), None)
    }

    /// The shared HTTP state of module `id`, created on first use.
    pub fn module(&self, id: &str) -> ModuleHttp {
        let mut modules = self.inner.modules.lock().unwrap_or_else(|e| e.into_inner());
        modules.entry(id.to_string()).or_default().clone()
    }

    /// A new session that uses `module`'s connection queue and cookie jar.
    pub fn session_for(&self, module: &ModuleHttp) -> HttpSession {
        HttpSession::new(self.clone(), Some(module.clone()))
    }

    pub(crate) fn defaults(&self) -> MutexGuard<'_, Defaults> {
        self.inner
            .defaults
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// A fresh stamp for a changed setting.
    pub(crate) fn next_generation(&self) -> u64 {
        self.inner.generation.fetch_add(1, Ordering::SeqCst)
    }

    fn apply<T: PartialEq>(&self, value: T, field: impl FnOnce(&mut Defaults) -> &mut Setting<T>) {
        let generation = self.next_generation();
        let mut defaults = self.defaults();
        let setting = field(&mut defaults);
        if setting.value != value {
            *setting = Setting { value, generation };
        }
    }

    /// `DefaultUserAgent` for sessions created from now on; a blank value leaves them
    /// with Synapse's own user agent (baseunits/httpsendthread.pas:164, 500-501).
    pub fn set_default_user_agent(&self, user_agent: impl Into<String>) {
        self.defaults().user_agent = user_agent.into();
    }

    /// `SetDefaultRetryCountAndApply` (baseunits/httpsendthread.pas:378-392): a changed
    /// value applies to new and existing sessions.
    pub fn set_default_retry_count(&self, retry_count: i32) {
        self.apply(retry_count, |d| &mut d.retry_count);
    }

    /// `SetDefaultTimeoutAndApply` in milliseconds (baseunits/httpsendthread.pas:362-376).
    pub fn set_default_timeout(&self, timeout_ms: u32) {
        self.apply(timeout_ms, |d| &mut d.timeout_ms);
    }

    /// `SetDefaultProxyAndApply` (baseunits/httpsendthread.pas:332-360); `None` for no proxy.
    pub fn set_default_proxy(&self, proxy: Option<Proxy>) {
        self.apply(proxy, |d| &mut d.proxy);
    }
}
