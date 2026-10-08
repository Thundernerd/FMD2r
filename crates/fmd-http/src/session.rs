//! One `THTTPSendThread`: request state plus the request algorithm.

use std::sync::Arc;
use std::time::Duration;

use crate::client::{HttpClient, Setting};
use crate::module::ModuleHttp;
use crate::strings::NameValueList;
use crate::terminate::TerminateToken;
use crate::transport::{Proxy, ProxyKind, WireRequest, WireResponse};
use crate::{HttpError, decode, url};

/// Headers, document and MIME type of a request, restored before each re-send.
struct RequestState {
    headers: NameValueList,
    document: Vec<u8>,
    mime_type: String,
}

/// FMD2's default user agent (baseunits/httpsendthread.pas:160).
pub const USER_AGENT_DEFAULT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";

/// Synapse's own user agent, used when the default is blank
/// (baseunits/synapse/httpsend.pas:311, baseunits/httpsendthread.pas:500-501).
pub const USER_AGENT_SYNAPSE: &str = "Mozilla/4.0 (compatible; Synapse)";

/// A hook run on a session while it prepares a request; see
/// [`HttpSession::set_on_after_set_cookies`].
pub type SessionHook = Arc<dyn Fn(&mut HttpSession) + Send + Sync>;

/// The Rust counterpart of one FMD2 `THTTPSendThread` (baseunits/httpsendthread.pas:73-140).
pub struct HttpSession {
    client: HttpClient,
    module: Option<ModuleHttp>,
    terminate: TerminateToken,
    headers: NameValueList,
    cookies: NameValueList,
    document: Vec<u8>,
    mime_type: String,
    result_code: i32,
    result_text: String,
    last_url: String,
    user_agent: String,
    retry_count: Setting<i32>,
    timeout_ms: Setting<u32>,
    proxy: Setting<Option<Proxy>>,
    compress: bool,
    follow_redirection: bool,
    max_redirect: u32,
    allow_server_error_response: bool,
    enabled_cookies: bool,
    clear_cookies: bool,
    after_set_cookies: Option<SessionHook>,
}

impl HttpSession {
    /// The constructor (baseunits/httpsendthread.pas:496-529): defaults from the client,
    /// cookies on, at most 5 redirects, then `Reset`.
    pub(crate) fn new(client: HttpClient, module: Option<ModuleHttp>) -> Self {
        let defaults = client.defaults().clone();
        let user_agent = if defaults.user_agent.trim().is_empty() {
            USER_AGENT_SYNAPSE.to_string()
        } else {
            defaults.user_agent
        };
        let mut session = Self {
            client,
            module,
            terminate: TerminateToken::new(),
            headers: NameValueList::new(':'),
            cookies: NameValueList::new('='),
            document: Vec::new(),
            mime_type: "text/html".into(),
            result_code: 0,
            result_text: String::new(),
            last_url: String::new(),
            user_agent,
            retry_count: defaults.retry_count,
            timeout_ms: defaults.timeout_ms,
            proxy: defaults.proxy,
            compress: true,
            follow_redirection: true,
            max_redirect: 5,
            allow_server_error_response: false,
            enabled_cookies: true,
            clear_cookies: false,
            after_set_cookies: None,
        };
        session.reset();
        session
    }

    /// `ResetBasic` (baseunits/httpsendthread.pas:934-946): clears the document, headers
    /// and cookies, sets `MimeType` to `text/html` and, with compression on, asks for
    /// `gzip, deflate, br, zstd`.
    pub fn reset_basic(&mut self) {
        self.document.clear();
        self.headers.clear();
        self.cookies.clear();
        self.mime_type = "text/html".into();
        if self.compress {
            self.headers
                .set_value("Accept-Encoding", "gzip, deflate, br, zstd");
        }
    }

    /// `Reset` (baseunits/httpsendthread.pas:924-932): `ResetBasic` plus browser-like
    /// default headers.
    pub fn reset(&mut self) {
        self.reset_basic();
        self.headers.set_value("DNT", "1");
        self.headers.set_value("Upgrade-Insecure-Requests", "1");
        self.headers.set_value(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
        );
        self.headers.set_value("Accept-Language", "en-US,en;q=0.5");
        self.headers.set_value("Accept-Charset", "utf-8");
    }

    /// True when `Headers` still holds a response, i.e. its text starts with `HTTP/`
    /// (baseunits/httpsendthread.pas:617).
    fn holds_response_headers(&self) -> bool {
        self.headers
            .lines()
            .first()
            .is_some_and(|l| l.starts_with("HTTP/"))
    }

    /// `NormalizeHeaders` (baseunits/httpsendthread.pas:460-466): every line becomes
    /// `Trim(Name): Trim(Value)`.
    fn normalize_headers(&mut self) {
        let lines = (0..self.headers.lines().len())
            .map(|i| {
                format!(
                    "{}: {}",
                    self.headers.name_at(i).trim(),
                    self.headers.value_at(i).trim()
                )
            })
            .collect();
        *self.headers.lines_mut() = lines;
    }

    /// `GET` (baseunits/httpsendthread.pas:725-728).
    pub fn get(&mut self, url: &str) -> Result<bool, HttpError> {
        self.request("GET", url)
    }

    /// `HEAD` (baseunits/httpsendthread.pas:720-723).
    pub fn head(&mut self, url: &str) -> Result<bool, HttpError> {
        self.request("HEAD", url)
    }

    /// `POST` (baseunits/httpsendthread.pas:730-747): non-empty `data` replaces the
    /// document; a `Content-Type` request header is moved into `MimeType`, and a
    /// `MimeType` of exactly `text/html` is sent as
    /// `application/x-www-form-urlencoded; charset=UTF-8`.
    pub fn post(&mut self, url: &str, data: &[u8]) -> Result<bool, HttpError> {
        if !data.is_empty() {
            self.document = data.to_vec();
        }
        if let Some(i) = self.headers.index_of_name("Content-Type") {
            self.mime_type = self.headers.value_at(i).trim().to_string();
            self.headers.remove(i);
        }
        if self.mime_type == "text/html" {
            self.mime_type = "application/x-www-form-urlencoded; charset=UTF-8".into();
        }
        self.request("POST", url)
    }

    /// `XHR` (baseunits/httpsendthread.pas:749-755): resets stale response headers, adds
    /// `X-Requested-With: XMLHttpRequest`, then GETs.
    pub fn xhr(&mut self, url: &str) -> Result<bool, HttpError> {
        if self.holds_response_headers() {
            self.reset();
        }
        self.headers
            .set_value("X-Requested-With", " XMLHttpRequest");
        self.get(url)
    }

    /// Runs `method` on `url` and returns whether the response body is non-empty
    /// (baseunits/httpsendthread.pas:718).
    pub fn request(&mut self, method: &str, url: &str) -> Result<bool, HttpError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(HttpError::InsideRuntime);
        }
        let handle = self.client.handle();
        Ok(handle.block_on(self.http_request(method, url)))
    }

    /// `HTTPRequest` (baseunits/httpsendthread.pas:578-590): nothing when terminated,
    /// otherwise the whole request holds one slot of the module's connection queue.
    async fn http_request(&mut self, method: &str, url: &str) -> bool {
        if self.terminate.is_terminated() {
            return false;
        }
        let _slot = match &self.module {
            Some(module) => match module.queue.acquire(&self.terminate).await {
                Some(slot) => Some(slot),
                None => return false,
            },
            None => None,
        };
        self.default_http_request(method, url).await
    }

    /// `DefaultHTTPRequest` (baseunits/httpsendthread.pas:592-719).
    async fn default_http_request(&mut self, method: &str, url: &str) -> bool {
        self.last_url = url::normalize(url);
        if self.last_url.is_empty() {
            return false;
        }
        if self.holds_response_headers() {
            self.reset();
        }
        self.normalize_headers();
        let saved = self.request_snapshot();
        if !self.send_with_retries(method, &saved).await {
            return false;
        }
        // Follow 301/302/303/307 as GET, at most MaxRedirect times; past that the
        // request fails. The first hop adds `Referer: <url before the redirect>` unless
        // one is set (baseunits/httpsendthread.pas:631-678).
        let mut headers = saved.headers;
        let mut redirects = 0;
        while self.follow_redirection && matches!(self.result_code, 301 | 302 | 303 | 307) {
            if self.terminate.is_terminated() {
                return false;
            }
            if redirects >= self.max_redirect {
                return false;
            }
            redirects += 1;
            if headers.index_of_name("Referer").is_none() {
                headers.push(format!("Referer: {}", self.last_url));
            }
            let location = self.headers.value("Location").trim().to_string();
            if !location.is_empty() {
                let (mut host, path) = url::split_url(&location);
                if host.is_empty() {
                    host = url::split_url(&self.last_url).0;
                }
                self.last_url = host + &path;
            }
            // Synapse `Clear`: no body, MimeType text/html (baseunits/synapse/httpsend.pas:346-355).
            let request = RequestState {
                headers: headers.clone(),
                document: Vec::new(),
                mime_type: "text/html".into(),
            };
            self.restore(&request);
            if !self.send_with_retries("GET", &request).await {
                return false;
            }
        }
        if !self.document.is_empty() {
            let encoding = self.headers.value("Content-Encoding").to_string();
            self.document = decode::decode(&encoding, std::mem::take(&mut self.document));
        }
        !self.document.is_empty()
    }

    /// Sends until success, retrying on transport errors and on status > 500 (unless
    /// `AllowServerErrorResponse`) up to RetryCount extra attempts; -1 retries forever
    /// (baseunits/httpsendthread.pas:623-628, 664-672). False when out of attempts.
    async fn send_with_retries(&mut self, method: &str, request: &RequestState) -> bool {
        let mut counter = 0;
        while !self.http_method(method).await
            || (!self.allow_server_error_response && self.result_code > 500)
        {
            if self.terminate.is_terminated() {
                return false;
            }
            let retry_count = self.retry_count();
            if retry_count > -1 && retry_count <= counter {
                return false;
            }
            counter += 1;
            self.restore(request);
        }
        true
    }

    /// Request state to re-send on a retry. FMD2 restores only the headers
    /// (baseunits/httpsendthread.pas:627); by then Synapse has replaced `Document` with
    /// the error response (baseunits/synapse/httpsend.pas:613), so a retried POST would
    /// upload the error page. We re-send the original body and MIME type instead.
    fn request_snapshot(&self) -> RequestState {
        RequestState {
            headers: self.headers.clone(),
            document: self.document.clone(),
            mime_type: self.mime_type.clone(),
        }
    }

    fn restore(&mut self, request: &RequestState) {
        self.headers = request.headers.clone();
        self.document = request.document.clone();
        self.mime_type = request.mime_type.clone();
    }

    /// One exchange, like Synapse's `THTTPSend.HTTPMethod`
    /// (baseunits/synapse/httpsend.pas:417-726). Returns false on a transport error, in
    /// which case `ResultCode` is 500.
    async fn http_method(&mut self, method: &str) -> bool {
        self.result_code = 500;
        self.result_text.clear();
        self.set_http_cookies();
        let request = self.wire_request(method);
        let transport = self.client.transport();
        let terminate = self.terminate.clone();
        // Termination closes the socket mid-request (`Stop`, baseunits/httpsendthread.pas:818-825).
        let response = tokio::select! {
            response = transport.send(request) => response,
            _ = terminate.terminated() => return false,
        };
        let sent = match response {
            Ok(response) => {
                self.take_response(response);
                true
            }
            Err(_) => false,
        };
        // Runs whether or not the exchange succeeded (baseunits/httpsendthread.pas:566-569).
        if self.enabled_cookies {
            self.parse_server_cookies();
        } else {
            self.cookies.clear();
        }
        sent
    }

    /// `SetHTTPCookies` (baseunits/httpsendthread.pas:468-479): adds the module jar's
    /// matching cookies, then runs the `OnAfterSetHTTPCookies` hook; right after
    /// `ClearCookies` it does neither, once.
    fn set_http_cookies(&mut self) {
        if self.clear_cookies {
            self.clear_cookies = false;
            return;
        }
        if self.enabled_cookies
            && let Some(module) = &self.module
        {
            module
                .cookies
                .set_cookies(&self.last_url, &mut self.cookies);
        }
        if let Some(hook) = self.after_set_cookies.clone() {
            hook(self);
        }
    }

    /// Sets `OnAfterSetHTTPCookies` (baseunits/httpsendthread.pas:477-478): a hook run before
    /// every exchange, after the module jar's cookies were added. FMD2 uses it to merge the
    /// module's settings cookies (baseunits/WebsiteModules.pas:278-282, :359).
    pub fn set_on_after_set_cookies(&mut self, hook: Option<SessionHook>) {
        self.after_set_cookies = hook;
    }

    /// Builds the request from the header list plus the lines Synapse inserts:
    /// User-Agent, Cookie, and Content-Type when a body is sent
    /// (baseunits/synapse/httpsend.pas:471-497).
    fn wire_request(&self, method: &str) -> WireRequest {
        let mut headers: Vec<(String, String)> = Vec::new();
        if !self.user_agent.is_empty() {
            headers.push(("User-Agent".into(), self.user_agent.clone()));
        }
        if !self.cookies.is_empty() {
            headers.push(("Cookie".into(), self.cookies.lines().join("; ")));
        }
        if !self.document.is_empty() && !self.mime_type.is_empty() {
            headers.push(("Content-Type".into(), self.mime_type.clone()));
        }
        for i in 0..self.headers.lines().len() {
            let name = self.headers.name_at(i).trim();
            if !name.is_empty() {
                headers.push((
                    name.to_string(),
                    self.headers.value_at(i).trim().to_string(),
                ));
            }
        }
        WireRequest {
            method: method.to_uppercase(),
            url: self.last_url.clone(),
            headers,
            body: self.document.clone(),
            timeout: Duration::from_millis(u64::from(self.timeout())),
            proxy: self.proxy(),
        }
    }

    /// Synapse's `Clear` followed by reading the response: `Headers` becomes the
    /// response's status line and headers, `MimeType` its Content-Type (default
    /// `text/html`) and `Document` its body (baseunits/synapse/httpsend.pas:346-355,
    /// 613-697).
    ///
    /// Unlike Synapse's raw lines, header names arrive lowercased from hyper and the
    /// status line and `ResultString` carry the canonical reason phrase rather than the
    /// server's. `Values[...]` lookups are case-insensitive, so modules see no difference.
    fn take_response(&mut self, response: WireResponse) {
        self.headers.clear();
        self.mime_type = "text/html".into();
        self.result_code = i32::from(response.status);
        self.result_text = response.reason.clone();
        let status = if response.reason.is_empty() {
            format!("HTTP/1.1 {}", response.status)
        } else {
            format!("HTTP/1.1 {} {}", response.status, response.reason)
        };
        self.headers.push(status);
        for (name, value) in &response.headers {
            if name.eq_ignore_ascii_case("content-type") {
                self.mime_type = value.trim().to_string();
            }
            self.headers.push(format!("{name}: {value}"));
        }
        self.document = response.body;
        // Synapse `ParseCookies` (baseunits/synapse/httpsend.pas:780-795).
        for (name, value) in &response.headers {
            if name.eq_ignore_ascii_case("set-cookie") {
                let pair = value.split(';').next().unwrap_or_default().trim();
                let (n, v) = pair.split_once('=').unwrap_or((pair, pair));
                self.cookies.set_value(n.trim(), v.trim());
            }
        }
    }

    /// Status code of the last response; 500 after a transport error (`ResultCode`,
    /// baseunits/synapse/httpsend.pas:215, 436).
    pub fn result_code(&self) -> i32 {
        self.result_code
    }

    /// Reason phrase of the last response (`ResultString`,
    /// baseunits/synapse/httpsend.pas:218, 357-367).
    pub fn result_text(&self) -> &str {
        &self.result_text
    }

    /// Response body of the last request, or the body to send (`Document`,
    /// baseunits/synapse/httpsend.pas:168).
    pub fn document(&self) -> &[u8] {
        &self.document
    }

    /// Mutable `Document`, e.g. to set a body before `request("PUT", …)`.
    pub fn document_mut(&mut self) -> &mut Vec<u8> {
        &mut self.document
    }

    /// `LastURL`: the URL of the last request, after normalisation and redirects
    /// (baseunits/httpsendthread.pas:128, 613-616, 657).
    pub fn last_url(&self) -> &str {
        &self.last_url
    }

    /// The token that terminates this session's requests; clone it to terminate from
    /// another thread.
    pub fn terminate_token(&self) -> TerminateToken {
        self.terminate.clone()
    }

    /// Ties this session to its owner's token, like FMD2 binding a `THTTPSendThread`
    /// to its owner thread (baseunits/httpsendthread.pas:520-525).
    pub fn set_terminate_token(&mut self, token: TerminateToken) {
        self.terminate = token;
    }

    /// `Terminated`: whether the owner was terminated (baseunits/httpsendthread.pas:810-816).
    pub fn terminated(&self) -> bool {
        self.terminate.is_terminated()
    }

    /// `Cookies`: the cookies sent with the next request, as `name=value` lines
    /// (baseunits/synapse/httpsend.pas:164, 488-497).
    pub fn cookies(&self) -> &NameValueList {
        &self.cookies
    }

    /// Mutable `Cookies`; see [`cookies`](Self::cookies).
    pub fn cookies_mut(&mut self) -> &mut NameValueList {
        &mut self.cookies
    }

    /// `GetCookies`: the session cookies joined by `; ` (baseunits/httpsendthread.pas:757-767).
    pub fn get_cookies(&self) -> String {
        self.cookies.lines().join("; ")
    }

    /// `MergeCookies` (baseunits/httpsendthread.pas:769-781): `name=value` pairs set or
    /// replace session cookies; bare words are added once.
    pub fn merge_cookies(&mut self, cookies: &str) {
        for part in cookies.split(';') {
            let part = part.trim();
            if let Some((name, value)) = part.split_once('=') {
                self.cookies.set_value(name.trim(), value.trim());
            } else if self.cookies.index_of(part).is_none() {
                self.cookies.push(part);
            }
        }
    }

    /// `RemoveCookie` (baseunits/httpsendthread.pas:827-836): removes a session cookie.
    pub fn remove_cookie(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        if let Some(i) = self.cookies.index_of_name(name) {
            self.cookies.remove(i);
        }
    }

    /// `AddServerCookies` (baseunits/httpsendthread.pas:783-788): stores `Set-Cookie`
    /// lines in the module's jar, dated now. Without a module there is no jar.
    pub fn add_server_cookies(&self, url: &str, cookies: &str) {
        if let Some(module) = &self.module {
            module
                .cookies
                .add_server_cookies(url, cookies, std::time::SystemTime::now());
        }
    }

    /// `ClearCookies` (baseunits/httpsendthread.pas:948-952): clears the session cookies
    /// and skips the jar for the next request.
    pub fn clear_cookies(&mut self) {
        self.cookies.clear();
        self.clear_cookies = true;
    }

    /// `ClearCookiesStorage` (baseunits/httpsendthread.pas:954-959): clears the session
    /// cookies and the module's jar.
    pub fn clear_cookies_storage(&mut self) {
        self.cookies.clear();
        if let Some(module) = &self.module {
            module.cookies.clear();
        }
    }

    /// `EnabledCookies`: when off, the jar is neither read nor written and response
    /// cookies are dropped (baseunits/httpsendthread.pas:475, 566-569).
    pub fn enabled_cookies(&self) -> bool {
        self.enabled_cookies
    }

    /// Sets `EnabledCookies`; see [`enabled_cookies`](Self::enabled_cookies).
    pub fn set_enabled_cookies(&mut self, enabled: bool) {
        self.enabled_cookies = enabled;
    }

    /// `MimeType`: Content-Type of the body to send, and of the last response
    /// (baseunits/synapse/httpsend.pas:181, 473-475, 669-670).
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// Sets `MimeType`; see [`mime_type`](Self::mime_type).
    pub fn set_mime_type(&mut self, mime_type: impl Into<String>) {
        self.mime_type = mime_type.into();
    }

    /// Request headers before a call, response headers after it (`Headers`,
    /// baseunits/synapse/httpsend.pas:158, 613-662).
    pub fn headers(&self) -> &NameValueList {
        &self.headers
    }

    /// Mutable `Headers`; see [`headers`](Self::headers).
    pub fn headers_mut(&mut self) -> &mut NameValueList {
        &mut self.headers
    }

    /// `RetryCount`: extra attempts after a failure; -1 retries until terminated
    /// (baseunits/httpsendthread.pas:624-626).
    pub fn retry_count(&self) -> i32 {
        self.retry_count
            .effective(&self.client.defaults().retry_count)
    }

    /// Sets `RetryCount` for this session; see [`retry_count`](Self::retry_count).
    pub fn set_retry_count(&mut self, retry_count: i32) {
        self.retry_count = self.stamp(retry_count);
    }

    /// `Timeout` in milliseconds: connect and per-read socket timeout
    /// (baseunits/httpsendthread.pas:444-451).
    pub fn timeout(&self) -> u32 {
        self.timeout_ms
            .effective(&self.client.defaults().timeout_ms)
    }

    /// Sets `Timeout` for this session; see [`timeout`](Self::timeout).
    pub fn set_timeout(&mut self, timeout_ms: u32) {
        self.timeout_ms = self.stamp(timeout_ms);
    }

    fn stamp<T>(&self, value: T) -> Setting<T> {
        Setting {
            value,
            generation: self.client.next_generation(),
        }
    }

    /// `SetProxy(type, host, port, user, pass)` (baseunits/httpsendthread.pas:838-874):
    /// `HTTP`, `SOCKS4` or `SOCKS5` (any case); any other type means no proxy.
    pub fn set_proxy(&mut self, kind: &str, host: &str, port: &str, user: &str, pass: &str) {
        let kind = match kind.to_uppercase().as_str() {
            "HTTP" => Some(ProxyKind::Http),
            "SOCKS4" => Some(ProxyKind::Socks4),
            "SOCKS5" => Some(ProxyKind::Socks5),
            _ => None,
        };
        let proxy = kind.map(|kind| Proxy {
            kind,
            host: host.into(),
            port: port.into(),
            user: user.into(),
            pass: pass.into(),
        });
        self.proxy = self.stamp(proxy);
    }

    /// `GetProxy` (baseunits/httpsendthread.pas:876-907); `None` without a proxy host.
    pub fn proxy(&self) -> Option<Proxy> {
        self.proxy
            .effective(&self.client.defaults().proxy)
            .filter(|p| !p.host.is_empty())
    }

    /// `ParseServerCookies` (baseunits/lua/LuaHTTPSend.pas:85-89, `ParseHTTPCookies`,
    /// baseunits/httpsendthread.pas:481-485): stores the `Set-Cookie` lines of the
    /// current `Headers` in the module's jar.
    pub fn parse_server_cookies(&self) {
        if let Some(module) = &self.module {
            module
                .cookies
                .add_response_cookies(&self.last_url, &self.headers);
        }
    }

    /// Sets `AllowServerErrorResponse`: when on, a status above 500 is a final answer
    /// instead of a reason to retry (baseunits/httpsendthread.pas:125, 623-624).
    pub fn set_allow_server_error_response(&mut self, allow: bool) {
        self.allow_server_error_response = allow;
    }

    /// Sets `FollowRedirection` (baseunits/httpsendthread.pas:124, 642).
    pub fn set_follow_redirection(&mut self, follow: bool) {
        self.follow_redirection = follow;
    }

    /// Sets `MaxRedirect`, 5 by default (baseunits/httpsendthread.pas:127, 516, 646).
    pub fn set_max_redirect(&mut self, max_redirect: u32) {
        self.max_redirect = max_redirect;
    }

    /// Sets `Compress`: whether `ResetBasic` asks for compressed responses
    /// (baseunits/httpsendthread.pas:123, 945).
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
    }

    /// `UserAgent`, sent as `User-Agent` when non-empty
    /// (baseunits/synapse/httpsend.pas:212, 478-479).
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Sets `UserAgent`; see [`user_agent`](Self::user_agent).
    pub fn set_user_agent(&mut self, user_agent: impl Into<String>) {
        self.user_agent = user_agent.into();
    }
}
