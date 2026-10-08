#![allow(dead_code)]
// each test binary uses a different subset of these helpers
// Test helpers live outside #[test] fns, which clippy.toml exempts; unwrap is fine in tests/.
#![allow(clippy::unwrap_used)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use fmd_http::{BoxFuture, Transport, TransportError, WireRequest, WireResponse};

/// A transport that records every request and answers from a script.
#[derive(Default)]
pub struct StubTransport {
    requests: Mutex<Vec<WireRequest>>,
    responses: Mutex<VecDeque<Result<WireResponse, TransportError>>>,
}

impl StubTransport {
    pub fn new(responses: Vec<Result<WireResponse, TransportError>>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
        })
    }

    pub fn requests(&self) -> Vec<WireRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Transport for StubTransport {
    fn send(
        &self,
        request: WireRequest,
    ) -> BoxFuture<'static, Result<WireResponse, TransportError>> {
        self.requests.lock().unwrap().push(request);
        let next = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(TransportError("no scripted response left".into())));
        Box::pin(async move { next })
    }
}

pub fn response(
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<WireResponse, TransportError> {
    Ok(WireResponse {
        status,
        reason: String::new(),
        headers: headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
        body: body.to_vec(),
    })
}

/// An axum server on 127.0.0.1 running on its own runtime, so test threads stay
/// outside any tokio context (as Lua worker threads are).
pub struct TestServer {
    pub base: String,
    _runtime: tokio::runtime::Runtime,
}

impl TestServer {
    pub fn start(router: axum::Router) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        runtime.spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            base,
            _runtime: runtime,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
}

/// Handler that answers with the request's method, then its headers as `name: value`
/// lines, then its body, plus an `X-Echo: yes` response header.
pub async fn echo(
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> impl axum::response::IntoResponse {
    let mut text = format!("{method}\n");
    for (name, value) in &headers {
        text.push_str(&format!("{}: {}\n", name, value.to_str().unwrap()));
    }
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&body));
    ([("X-Echo", "yes")], text)
}

/// The request header `name` as echoed by [`echo`].
pub fn echoed_header(document: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(document);
    text.lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .find_map(|l| {
            let (n, v) = l.split_once(": ")?;
            n.eq_ignore_ascii_case(name).then(|| v.to_string())
        })
}
