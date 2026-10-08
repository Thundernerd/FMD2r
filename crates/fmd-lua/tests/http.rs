//! The `HTTP` object, exercised through Lua snippets on the public runtime with an `HTTP` global
//! bound to a session over a stub transport (docs/tickets/T10-http-lua-object.md, "Seams under
//! test").
//!
//! Expected values come from FMD2's `THTTPSendThread` (baseunits/httpsendthread.pas) and its Lua
//! wrapper (baseunits/lua/LuaHTTPSend.pas).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use fmd_http::{
    BoxFuture, HttpClient, Proxy, ProxyKind, Transport, TransportError, USER_AGENT_DEFAULT,
    WireRequest, WireResponse,
};
use fmd_lua::{
    HttpModule, LuaHttp, ModuleHttpOverrides, ModuleHttpSettings, ProxyOverride, Runtime,
    SettingsStoreError, create_http,
};

/// A transport that records every request and answers from a script.
#[derive(Default)]
struct StubTransport {
    requests: Mutex<Vec<WireRequest>>,
    responses: Mutex<VecDeque<WireResponse>>,
}

impl StubTransport {
    fn new(responses: Vec<WireResponse>) -> Arc<Self> {
        Arc::new(StubTransport {
            requests: Mutex::default(),
            responses: Mutex::new(responses.into()),
        })
    }

    fn requests(&self) -> Vec<WireRequest> {
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
            .ok_or_else(|| TransportError("no scripted response left".into()));
        Box::pin(async move { next })
    }
}

fn response(status: u16, headers: &[(&str, &str)], body: &[u8]) -> WireResponse {
    WireResponse {
        status,
        reason: String::new(),
        headers: headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
        body: body.to_vec(),
    }
}

/// The value of header `name` in `request`.
fn header<'a>(request: &'a WireRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// A runtime with an `HTTP` global over a session (not bound to a module) of a client that
/// answers from `responses`.
fn runtime_with_http(responses: Vec<WireResponse>) -> (Runtime, Arc<StubTransport>) {
    let stub = StubTransport::new(responses);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let rt = Runtime::new().unwrap();
    let http = LuaHttp::new(client.session()).build(rt.lua()).unwrap();
    rt.lua().globals().set("HTTP", http).unwrap();
    (rt, stub)
}

#[test]
fn get_returns_true_on_a_404_with_a_body_and_headers_hold_the_response() {
    let (rt, stub) = runtime_with_http(vec![response(
        404,
        &[("Content-Type", "text/plain")],
        b"nope",
    )]);
    rt.exec(
        r#"
        HTTP.Headers.Values['X-Test'] = '1'
        assert(HTTP.GET('example.test/404-with-body') == true)
        assert(HTTP.ResultCode == 404)
        assert(HTTP.Headers.Values['Content-Type'] ~= '')
        assert(HTTP.Headers[0] == 'HTTP/1.1 404')
        assert(HTTP.Document.ToString() == 'nope')
        "#,
    )
    .unwrap();
    assert_eq!(header(&stub.requests()[0], "X-Test"), Some("1"));
}

#[test]
fn the_next_request_resets_headers_that_still_hold_a_response() {
    let (rt, stub) =
        runtime_with_http(vec![response(200, &[], b"one"), response(200, &[], b"two")]);
    rt.exec(
        r#"
        HTTP.Headers.Values['X-Test'] = '1'
        HTTP.GET('example.test/a')
        -- Headers now start with the status line, so this line is dropped by the reset.
        HTTP.Headers.Values['X-Late'] = '1'
        HTTP.GET('example.test/b')
        "#,
    )
    .unwrap();
    let second = &stub.requests()[1];
    assert_eq!(header(second, "X-Test"), None);
    assert_eq!(header(second, "X-Late"), None);
    assert_eq!(header(second, "DNT"), Some("1"));
}

#[test]
fn post_sends_a_text_html_content_type_header_as_form_urlencoded() {
    let (rt, stub) = runtime_with_http(vec![
        response(200, &[], b"page"),
        response(200, &[], b"posted"),
    ]);
    rt.exec(
        r#"
        HTTP.GET('example.test/form')
        HTTP.Reset()
        HTTP.Headers.Values['Content-Type'] = 'text/html'
        assert(HTTP.POST('example.test/post', 'a=1') == true)
        "#,
    )
    .unwrap();
    let post = &stub.requests()[1];
    assert_eq!(post.method, "POST");
    assert_eq!(post.body, b"a=1");
    assert_eq!(
        header(post, "Content-Type"),
        Some("application/x-www-form-urlencoded; charset=UTF-8")
    );
}

#[test]
fn post_moves_a_content_type_header_into_mime_type() {
    let (rt, stub) = runtime_with_http(vec![response(200, &[], b"posted")]);
    rt.exec(
        r#"
        HTTP.Headers.Values['Content-Type'] = 'application/json'
        HTTP.POST('example.test/api', '{}')
        "#,
    )
    .unwrap();
    let post = &stub.requests()[0];
    let content_types: Vec<_> = post
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("Content-Type"))
        .collect();
    assert_eq!(content_types.len(), 1);
    assert_eq!(content_types[0].1, "application/json");
}

#[test]
fn post_sends_mime_type_and_the_current_document_without_data() {
    let (rt, stub) = runtime_with_http(vec![response(200, &[], b"posted")]);
    rt.exec(
        r#"
        HTTP.MimeType = 'application/json'
        HTTP.Document.WriteString('{"k":1}')
        HTTP.POST('example.test/api')
        assert(HTTP.MimeType == 'text/html')  -- the response's, which has no Content-Type
        "#,
    )
    .unwrap();
    let post = &stub.requests()[0];
    assert_eq!(post.body, br#"{"k":1}"#);
    assert_eq!(header(post, "Content-Type"), Some("application/json"));
}

/// The first bytes of a PNG file: high bytes, a NUL and CR/LF.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\xff\xfe";

#[test]
fn document_is_binary_safe() {
    let (rt, stub) = runtime_with_http(vec![
        response(200, &[("Content-Type", "image/png")], PNG),
        response(200, &[], b"ok"),
    ]);
    rt.exec(
        r#"
        HTTP.GET('example.test/cover.png')
        assert(HTTP.Document.Size == 22)
        assert(HTTP.MimeType == 'image/png')
        local image = HTTP.Document.ToString()
        assert(image == '\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\1\xff\xfe')
        HTTP.Reset()
        HTTP.MimeType = 'image/png'
        HTTP.POST('example.test/upload', image)
        "#,
    )
    .unwrap();
    assert_eq!(stub.requests()[1].body, PNG);
}

#[test]
fn methods_take_colon_and_dot_calls() {
    let (rt, stub) = runtime_with_http(vec![
        response(200, &[], b"same"),
        response(200, &[], b"same"),
    ]);
    rt.exec("assert(HTTP:GET('example.test/x') == HTTP.GET('example.test/x'))")
        .unwrap();
    let urls: Vec<_> = stub.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls, ["https://example.test/x", "https://example.test/x"]);
}

#[test]
fn every_method_of_lua_http_send_exists() {
    let (rt, _) = runtime_with_http(vec![]);
    rt.exec(
        r#"
        for _, name in ipairs({'Request', 'GET', 'POST', 'HEAD', 'XHR', 'Reset', 'ResetBasic',
            'ClearCookies', 'ClearCookiesStorage', 'GetCookies', 'AddServerCookies',
            'ParseServerCookies', 'SetProxy'}) do
          assert(type(HTTP[name]) == 'function', name)
        end
        "#,
    )
    .unwrap();
}

#[test]
fn request_head_and_xhr_send_their_methods() {
    let (rt, stub) = runtime_with_http(vec![
        response(200, &[], b"put"),
        response(200, &[("Content-Length", "4")], b""),
        response(200, &[], b"{}"),
    ]);
    rt.exec(
        r#"
        assert(HTTP.Request('put', 'example.test/r') == true)
        assert(HTTP.HEAD('example.test/h') == false)  -- no body
        assert(HTTP.XHR('example.test/x') == true)
        "#,
    )
    .unwrap();
    let requests = stub.requests();
    assert_eq!(requests[0].method, "PUT");
    assert_eq!(requests[1].method, "HEAD");
    assert_eq!(requests[2].method, "GET");
    assert_eq!(
        header(&requests[2], "X-Requested-With"),
        Some("XMLHttpRequest")
    );
    // XHR first resets the response headers left by HEAD.
    assert_eq!(header(&requests[2], "DNT"), Some("1"));
}

#[test]
fn reset_basic_keeps_only_accept_encoding() {
    let (rt, _) = runtime_with_http(vec![response(200, &[], b"body")]);
    rt.exec(
        r#"
        HTTP.GET('example.test/x')
        HTTP.ResetBasic()
        assert(HTTP.Headers.Count == 1)
        assert(HTTP.Headers.Values['Accept-Encoding'] == 'gzip, deflate, br, zstd')
        assert(HTTP.Document.Size == 0)
        assert(HTTP.MimeType == 'text/html')
        "#,
    )
    .unwrap();
}

#[test]
fn read_only_properties_report_the_last_request() {
    let (rt, _) = runtime_with_http(vec![WireResponse {
        reason: "Not Found".into(),
        ..response(404, &[], b"")
    }]);
    rt.exec(
        r#"
        assert(HTTP.Terminated == false)
        assert(HTTP.LastURL == '')
        HTTP.GET('example.test/missing')
        assert(HTTP.LastURL == 'https://example.test/missing')
        assert(HTTP.ResultCode == 404)
        assert(HTTP.ResultString == 'Not Found')
        HTTP.ResultCode = 200; HTTP.LastURL = 'x'; HTTP.Terminated = true
        assert(HTTP.ResultCode == 404 and HTTP.LastURL ~= 'x' and HTTP.Terminated == false)
        "#,
    )
    .unwrap();
}

#[test]
fn user_agent_and_retry_count_apply_to_requests() {
    let (rt, stub) =
        runtime_with_http(vec![response(503, &[], b"busy"), response(200, &[], b"ok")]);
    rt.exec(
        r#"
        HTTP.UserAgent = 'TestAgent/1.0'
        assert(HTTP.UserAgent == 'TestAgent/1.0')
        HTTP.RetryCount = '1'  -- converted like lua_tointeger
        assert(HTTP.RetryCount == 1)
        assert(HTTP.GET('example.test/x') == true)
        "#,
    )
    .unwrap();
    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(header(&requests[0], "User-Agent"), Some("TestAgent/1.0"));
}

#[test]
fn response_cookies_land_in_cookies_unless_cookies_are_disabled() {
    let (rt, stub) = runtime_with_http(vec![
        response(200, &[("Set-Cookie", "sid=abc; Path=/")], b"one"),
        response(200, &[], b"two"),
        response(200, &[("Set-Cookie", "other=1")], b"three"),
    ]);
    rt.exec(
        r#"
        assert(HTTP.EnabledCookies == true)
        HTTP.GET('example.test/login')
        assert(HTTP.Cookies.Values['sid'] == 'abc')
        HTTP.Cookies.Values['lang'] = 'en'
        assert(HTTP.GetCookies() == 'sid=abc; lang=en')
        HTTP.GET('example.test/next')
        HTTP.ClearCookies()
        assert(HTTP.Cookies.Count == 0 and HTTP.GetCookies() == '')
        HTTP.EnabledCookies = false
        HTTP.GET('example.test/more')
        assert(HTTP.Cookies.Count == 0)
        "#,
    )
    .unwrap();
    // Without a module jar the next request's reset (`ResetBasic`) drops the session cookies
    // (baseunits/httpsendthread.pas:617, :934-946).
    assert_eq!(header(&stub.requests()[1], "Cookie"), None);
}

#[test]
fn set_proxy_routes_requests_through_the_proxy() {
    let (rt, stub) = runtime_with_http(vec![
        response(200, &[], b"via proxy"),
        response(200, &[], b"direct"),
    ]);
    rt.exec(
        r#"
        HTTP.SetProxy('socks5', '127.0.0.1', '1080', 'user', 'pass')
        HTTP.GET('example.test/a')
        HTTP.SetProxy('', '', '', '', '')
        HTTP.GET('example.test/b')
        "#,
    )
    .unwrap();
    let requests = stub.requests();
    assert_eq!(
        requests[0].proxy,
        Some(Proxy {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: "1080".into(),
            user: "user".into(),
            pass: "pass".into(),
        })
    );
    assert_eq!(requests[1].proxy, None);
}

/// Module HTTP settings that tests change between requests.
#[derive(Default)]
struct Settings(Mutex<Option<ModuleHttpOverrides>>);

impl ModuleHttpSettings for Settings {
    fn http_overrides(&self) -> Option<ModuleHttpOverrides> {
        self.0.lock().unwrap().clone()
    }

    fn clear_cookies(&self) -> Result<(), SettingsStoreError> {
        unreachable!("these sessions have no anti-bot hook")
    }

    fn store_bypass(&self, _: &str, _: &str) -> Result<(), SettingsStoreError> {
        unreachable!("these sessions have no anti-bot hook")
    }
}

/// A runtime with `HTTP` and `HTTP2` globals, both created for module `m` of one client.
fn runtime_with_module_http(
    responses: Vec<WireResponse>,
    settings: Arc<Settings>,
) -> (Runtime, Arc<StubTransport>, HttpClient) {
    let stub = StubTransport::new(responses);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let module = HttpModule {
        http: client.module("m"),
        settings,
    };
    let rt = Runtime::new().unwrap();
    for name in ["HTTP", "HTTP2"] {
        let session = create_http(&client, Some(&module));
        let http = LuaHttp::new(session).build(rt.lua()).unwrap();
        rt.lua().globals().set(name, http).unwrap();
    }
    (rt, stub, client)
}

#[test]
fn http_objects_of_one_module_share_its_cookie_jar() {
    let (rt, stub, client) = runtime_with_module_http(
        vec![
            response(200, &[("Set-Cookie", "sid=abc; Path=/")], b"login"),
            response(200, &[], b"page"),
            response(200, &[], b"other module"),
        ],
        Arc::default(),
    );
    rt.exec(
        r#"
        HTTP.GET('example.test/login')
        HTTP2.GET('example.test/page')
        "#,
    )
    .unwrap();
    assert_eq!(header(&stub.requests()[1], "Cookie"), Some("sid=abc"));

    let mut other = create_http(&client, None);
    other.get("example.test/page").unwrap();
    assert_eq!(header(&stub.requests()[2], "Cookie"), None);
}

#[test]
fn cookie_storage_methods_reach_the_module_jar() {
    let (rt, stub, _) = runtime_with_module_http(
        vec![
            response(200, &[], b"one"),
            response(200, &[], b"two"),
            response(200, &[("Set-Cookie", "parsed=1; Path=/")], b"three"),
            response(200, &[], b"four"),
        ],
        Arc::default(),
    );
    rt.exec(
        r#"
        HTTP.AddServerCookies('https://example.test/', 'a=1; Path=/')
        HTTP.AddServerCookies('b=2; Domain=example.test; Path=/')
        HTTP2.GET('example.test/x')
        HTTP.ClearCookiesStorage()
        HTTP2.GET('example.test/y')
        HTTP.EnabledCookies = false  -- keep the response's cookie out of the jar
        HTTP.GET('example.test/z')
        HTTP.EnabledCookies = true
        HTTP.ParseServerCookies()
        HTTP2.GET('example.test/w')
        "#,
    )
    .unwrap();
    let requests = stub.requests();
    assert_eq!(header(&requests[0], "Cookie"), Some("a=1; b=2"));
    assert_eq!(header(&requests[1], "Cookie"), None);
    assert_eq!(header(&requests[3], "Cookie"), Some("parsed=1"));
}

#[test]
fn module_settings_override_user_agent_and_merge_their_cookies_into_every_request() {
    let settings = Arc::new(Settings::default());
    *settings.0.lock().unwrap() = Some(ModuleHttpOverrides {
        user_agent: "ModuleAgent".into(),
        cookies: "cf_clearance=x; lang=en".into(),
        proxy: ProxyOverride::Default,
    });
    let (rt, stub, _) = runtime_with_module_http(
        vec![response(200, &[], b"one"), response(200, &[], b"two")],
        settings.clone(),
    );
    rt.exec("assert(HTTP.UserAgent == 'ModuleAgent'); HTTP.GET('example.test/a')")
        .unwrap();
    settings.0.lock().unwrap().as_mut().unwrap().cookies = "cf_clearance=y".into();
    rt.exec("HTTP.GET('example.test/b')").unwrap();
    let requests = stub.requests();
    assert_eq!(header(&requests[0], "User-Agent"), Some("ModuleAgent"));
    assert_eq!(
        header(&requests[0], "Cookie"),
        Some("cf_clearance=x; lang=en")
    );
    assert_eq!(header(&requests[1], "Cookie"), Some("cf_clearance=y"));
}

#[test]
fn disabled_module_settings_are_not_applied() {
    let (rt, stub, _) = runtime_with_module_http(vec![response(200, &[], b"one")], Arc::default());
    rt.exec("HTTP.GET('example.test/a')").unwrap();
    let request = &stub.requests()[0];
    assert_eq!(header(request, "User-Agent"), Some(USER_AGENT_DEFAULT));
    assert_eq!(header(request, "Cookie"), None);
}

#[test]
fn module_proxy_settings_replace_the_default_proxy() {
    let proxy = Proxy {
        kind: ProxyKind::Http,
        host: "proxy.test".into(),
        port: "3128".into(),
        user: String::new(),
        pass: String::new(),
    };
    let stub = StubTransport::new(vec![
        response(200, &[], b"direct"),
        response(200, &[], b"module proxy"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    client.set_default_proxy(Some(Proxy {
        host: "default.test".into(),
        ..proxy.clone()
    }));
    for proxy in [ProxyOverride::Direct, ProxyOverride::Proxy(proxy.clone())] {
        let module = HttpModule {
            http: client.module("m"),
            settings: Arc::new(Settings(Mutex::new(Some(ModuleHttpOverrides {
                proxy,
                ..ModuleHttpOverrides::default()
            })))),
        };
        create_http(&client, Some(&module))
            .get("example.test/a")
            .unwrap();
    }
    let requests = stub.requests();
    assert_eq!(requests[0].proxy, None);
    assert_eq!(requests[1].proxy, Some(proxy));
}

#[test]
fn only_requests_rewind_the_document() {
    let (rt, _) = runtime_with_http(vec![response(200, &[], b"body")]);
    rt.exec(
        r#"
        HTTP.Document.WriteString('abc')
        HTTP.GetCookies(); HTTP.ClearCookies(); HTTP.ParseServerCookies()
        assert(HTTP.Document.ToString() == '')  -- still at the end
        HTTP.GET('example.test/x')
        assert(HTTP.Document.ToString() == 'body')
        "#,
    )
    .unwrap();
}
