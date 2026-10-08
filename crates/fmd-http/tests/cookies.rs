// Helpers outside #[test] fns are not covered by clippy.toml's test exemption; unwrap is fine in tests/.
#![allow(clippy::unwrap_used)]

mod common;

use axum::Router;
use axum::http::{StatusCode, header};
use axum::routing::{any, get};
use common::{StubTransport, TestServer, echo, echoed_header, response};
use fmd_http::HttpClient;

fn server() -> TestServer {
    TestServer::start(
        Router::new()
            .route("/echo", any(echo))
            .route(
                "/set",
                get(|| async { ([(header::SET_COOKIE, "sid=abc; Path=/")], "set") }),
            )
            .route(
                "/expire",
                get(|| async {
                    (
                        [(header::SET_COOKIE, "sid=gone; Max-Age=0; Path=/")],
                        "expired",
                    )
                }),
            )
            .route(
                "/login",
                get(|| async {
                    (
                        StatusCode::FOUND,
                        [
                            (header::SET_COOKIE, "sid=abc; Path=/"),
                            (header::LOCATION, "/echo"),
                        ],
                    )
                }),
            ),
    )
}

#[test]
fn server_cookies_are_shared_by_sessions_of_the_same_module() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    let mut a = client.session_for(&module);
    let mut b = client.session_for(&module);

    a.get(&server.url("/set")).unwrap();
    b.get(&server.url("/echo")).unwrap();

    assert_eq!(
        echoed_header(b.document(), "cookie").as_deref(),
        Some("sid=abc")
    );
}

#[test]
fn other_modules_do_not_see_the_cookies() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let mut a = client.session_for(&client.module("a"));
    let mut b = client.session_for(&client.module("b"));

    a.get(&server.url("/set")).unwrap();
    b.get(&server.url("/echo")).unwrap();

    assert_eq!(echoed_header(b.document(), "cookie"), None);
}

#[test]
fn cookies_set_during_a_redirect_are_sent_on_the_next_hop() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session_for(&client.module("m"));

    session.get(&server.url("/login")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "cookie").as_deref(),
        Some("sid=abc")
    );
}

#[test]
fn response_cookies_are_parsed_into_the_session_cookies() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session();

    session.get(&server.url("/set")).unwrap();

    assert_eq!(session.cookies().value("sid"), "abc");
    assert_eq!(session.get_cookies(), "sid=abc");
}

#[test]
fn disabled_cookies_are_neither_stored_nor_kept() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    let mut a = client.session_for(&module);
    a.set_enabled_cookies(false);
    let mut b = client.session_for(&module);

    a.get(&server.url("/set")).unwrap();
    b.get(&server.url("/echo")).unwrap();

    assert!(a.cookies().is_empty());
    assert_eq!(echoed_header(b.document(), "cookie"), None);
}

#[test]
fn expired_cookies_are_not_sent() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    let mut session = client.session_for(&module);
    session.get(&server.url("/set")).unwrap();
    session.get(&server.url("/expire")).unwrap();

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(echoed_header(session.document(), "cookie"), None);
}

#[test]
fn add_server_cookies_feeds_the_module_jar() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    let mut session = client.session_for(&module);
    session.add_server_cookies(&server.url("/"), "a=1; path=/\nb=2; path=/");

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "cookie").as_deref(),
        Some("a=1; b=2")
    );
}

#[test]
fn merge_and_remove_edit_the_session_cookies() {
    let client = HttpClient::with_transport(StubTransport::new(vec![])).unwrap();
    let mut session = client.session();
    session.merge_cookies("a=1; b = 2 ;flag");
    session.merge_cookies("a=3");

    assert_eq!(session.get_cookies(), "a=3; b=2; flag");
    session.remove_cookie("b");
    assert_eq!(session.get_cookies(), "a=3; flag");
}

#[test]
fn clear_cookies_skips_the_jar_for_one_request() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session_for(&client.module("m"));
    session.get(&server.url("/set")).unwrap();
    session.clear_cookies();

    session.get(&server.url("/echo")).unwrap();
    assert_eq!(echoed_header(session.document(), "cookie"), None);

    session.get(&server.url("/echo")).unwrap();
    assert_eq!(
        echoed_header(session.document(), "cookie").as_deref(),
        Some("sid=abc")
    );
}

#[test]
fn clear_cookies_storage_empties_the_module_jar() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let module = client.module("m");
    let mut session = client.session_for(&module);
    session.get(&server.url("/set")).unwrap();
    session.clear_cookies_storage();

    session.get(&server.url("/echo")).unwrap();

    assert_eq!(echoed_header(session.document(), "cookie"), None);
    assert_eq!(module.cookies().get_server_cookies("127.0.0.1", ""), "");
}

#[test]
fn the_cookie_jar_round_trips_through_serialisation() {
    let server = server();
    let client = HttpClient::new().unwrap();
    let mut session = client.session_for(&client.module("m"));
    session.get(&server.url("/set")).unwrap();
    let saved = client.module("m").cookies().to_json().unwrap();

    let restored = HttpClient::new().unwrap();
    restored.module("m").cookies().load_json(&saved).unwrap();
    let mut session = restored.session_for(&restored.module("m"));
    session.get(&server.url("/echo")).unwrap();

    assert_eq!(
        echoed_header(session.document(), "cookie").as_deref(),
        Some("sid=abc")
    );
}

#[test]
fn get_server_cookies_lists_a_domains_cookies_in_set_cookie_form() {
    let client = HttpClient::with_transport(StubTransport::new(vec![])).unwrap();
    let module = client.module("m");
    let jar = module.cookies();
    jar.add_server_cookies(
        "https://example.test/a/b",
        "sid=1; Domain=.Example.test; HttpOnly; SameSite=Lax\nother=2",
        std::time::SystemTime::now(),
    );

    assert_eq!(
        jar.get_server_cookies("example.test", ""),
        "sid=1; domain=example.test; path=/a/b; httponly; samesite=lax\r\nother=2; domain=example.test; path=/a/b"
    );
    jar.remove_cookies("example.test", "SID");
    assert_eq!(
        jar.get_server_cookies("example.test", ""),
        "other=2; domain=example.test; path=/a/b"
    );
}

#[test]
fn cookies_match_parent_domains_and_path_prefixes() {
    let stub = StubTransport::new(vec![response(200, &[], b"ok"), response(200, &[], b"ok")]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let module = client.module("m");
    module.cookies().add_server_cookies(
        "https://example.test/",
        "d=1; domain=example.test; path=/\np=2; domain=example.test; path=/manga/",
        std::time::SystemTime::now(),
    );
    let mut session = client.session_for(&module);

    session.get("https://img.example.test/manga/1").unwrap();
    session.get("https://notexample.test/manga/1").unwrap();

    let cookie = |i: usize| {
        stub.requests()[i]
            .headers
            .iter()
            .find(|(n, _)| n == "Cookie")
            .map(|(_, v)| v.clone())
    };
    assert_eq!(cookie(0).as_deref(), Some("d=1; p=2"));
    assert_eq!(cookie(1), None);
}

#[test]
fn expires_dates_are_parsed_in_netscape_and_rfc_1123_forms() {
    let client = HttpClient::with_transport(StubTransport::new(vec![])).unwrap();
    let module = client.module("m");
    let jar = module.cookies();
    let now = std::time::SystemTime::now();
    jar.add_server_cookies(
        "https://example.test/",
        "a=1; path=/; expires=Wed, 21-Oct-37 07:28:00 GMT",
        now,
    );
    jar.add_server_cookies(
        "https://example.test/",
        "b=2; path=/; Expires=Sun, 06 Nov 1994 08:49:37 GMT",
        now,
    );

    assert_eq!(
        jar.get_server_cookies("example.test", "a"),
        "a=1; domain=example.test; path=/; expires=Wed, 21 Oct 2037 07:28:00 +0000"
    );
    let mut session = client.session_for(&module);
    session.get("example.test/").unwrap_or_default();
    assert_eq!(session.get_cookies(), "a=1");
}
