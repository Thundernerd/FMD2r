// Helpers outside #[test] fns are not covered by clippy.toml's test exemption; unwrap is fine in tests/.
#![allow(clippy::unwrap_used)]

mod common;

use common::{StubTransport, response};
use fmd_http::HttpClient;

#[test]
fn url_without_scheme_is_requested_over_https() {
    let stub = StubTransport::new(vec![response(200, &[], b"ok")]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();

    assert!(session.get("example.test/x").unwrap());

    assert_eq!(stub.requests()[0].url, "https://example.test/x");
}

#[test]
fn server_errors_above_500_are_retried_up_to_retry_count() {
    let stub = StubTransport::new(vec![
        response(502, &[], b"bad gateway"),
        response(502, &[], b"bad gateway"),
        response(200, &[], b"ok"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_retry_count(3);

    assert!(session.get("example.test/x").unwrap());

    assert_eq!(stub.requests().len(), 3);
    assert_eq!(session.result_code(), 200);
    assert_eq!(session.document(), b"ok");
}

#[test]
fn status_500_is_not_retried() {
    let stub = StubTransport::new(vec![
        response(500, &[], b"error page"),
        response(200, &[], b"ok"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_retry_count(3);

    assert!(session.get("example.test/x").unwrap());

    assert_eq!(stub.requests().len(), 1);
    assert_eq!(session.result_code(), 500);
}

#[test]
fn exhausted_retries_return_false_even_with_a_body() {
    let stub = StubTransport::new(vec![
        response(503, &[], b"busy"),
        response(503, &[], b"busy"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_retry_count(1);

    assert!(!session.get("example.test/x").unwrap());

    assert_eq!(stub.requests().len(), 2);
    assert_eq!(session.result_code(), 503);
}

#[test]
fn transport_errors_are_retried() {
    let stub = StubTransport::new(vec![
        Err(fmd_http::TransportError("connection reset".into())),
        response(200, &[], b"ok"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_retry_count(1);

    assert!(session.get("example.test/x").unwrap());
    assert_eq!(stub.requests().len(), 2);
}

fn url_requested_for(url: &str) -> String {
    let stub = StubTransport::new(vec![response(200, &[], b"ok")]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    client.session().get(url).unwrap();
    stub.requests()[0].url.clone()
}

#[test]
fn leading_slashes_colons_and_blanks_are_trimmed() {
    assert_eq!(
        url_requested_for("  ://example.test/x "),
        "https://example.test/x"
    );
    assert_eq!(
        url_requested_for("//example.test/x"),
        "https://example.test/x"
    );
}

#[test]
fn unencoded_urls_are_url_encoded() {
    assert_eq!(
        url_requested_for("example.test/a b/ü"),
        "https://example.test/a%20b/%C3%BC"
    );
}

#[test]
fn already_encoded_urls_are_left_alone() {
    assert_eq!(
        url_requested_for("example.test/a%20b c"),
        "https://example.test/a%20b c"
    );
}

#[test]
fn an_empty_url_sends_nothing() {
    let stub = StubTransport::new(vec![]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();

    assert!(!client.session().get(" // ").unwrap());
    assert!(stub.requests().is_empty());
}

fn redirect_target(location: &str) -> String {
    let stub = StubTransport::new(vec![
        response(302, &[("Location", location)], b""),
        response(200, &[], b"ok"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    client
        .session()
        .get("https://example.test/dir/page?q=1")
        .unwrap();
    stub.requests()[1].url.clone()
}

#[test]
fn redirect_locations_resolve_like_fmd2_split_url() {
    assert_eq!(
        redirect_target("https://cdn.example.test/img/1.jpg"),
        "https://cdn.example.test/img/1.jpg"
    );
    assert_eq!(
        redirect_target("http://other.test:8080/p"),
        "http://other.test:8080/p"
    );
    assert_eq!(
        redirect_target("//cdn.example.test/x"),
        "https://cdn.example.test/x"
    );
    assert_eq!(
        redirect_target("/root?a=1"),
        "https://example.test/root?a=1"
    );
    // Relative paths resolve against the host, not the current directory.
    assert_eq!(redirect_target("next"), "https://example.test/next");
}

#[test]
fn a_retried_post_resends_its_body() {
    let stub = StubTransport::new(vec![
        response(503, &[("Content-Type", "text/plain")], b"busy"),
        response(200, &[], b"ok"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_retry_count(1);

    assert!(session.post("example.test/form", b"a=1").unwrap());

    let retried = &stub.requests()[1];
    assert_eq!(retried.method, "POST");
    assert_eq!(retried.body, b"a=1");
    assert!(retried.headers.contains(&(
        "Content-Type".into(),
        "application/x-www-form-urlencoded; charset=UTF-8".into()
    )));
}

#[test]
fn a_post_after_an_unreset_request_loses_its_body() {
    // The stale response headers make the request Reset, which clears the document
    // POST just filled (baseunits/httpsendthread.pas:617, 736).
    let stub = StubTransport::new(vec![
        response(200, &[], b"first"),
        response(200, &[], b"second"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.get("example.test/").unwrap();

    session.post("example.test/form", b"a=1").unwrap();

    assert!(stub.requests()[1].body.is_empty());
}

#[test]
fn blocking_calls_are_refused_inside_a_tokio_runtime() {
    let client =
        HttpClient::with_transport(StubTransport::new(vec![response(200, &[], b"ok")])).unwrap();
    let mut session = client.session();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    let result = runtime.block_on(async { session.get("example.test/") });

    assert!(matches!(result, Err(fmd_http::HttpError::InsideRuntime)));
}

#[test]
fn allow_server_error_response_stops_retrying_5xx() {
    let stub = StubTransport::new(vec![
        response(503, &[], b"challenge"),
        response(200, &[], b"ok"),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_retry_count(3);
    session.set_allow_server_error_response(true);

    assert!(session.get("example.test/").unwrap());

    assert_eq!(stub.requests().len(), 1);
    assert_eq!(session.result_code(), 503);
}

#[test]
fn redirects_are_not_followed_when_follow_redirection_is_off() {
    let stub = StubTransport::new(vec![response(302, &[("Location", "/next")], b"moved")]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_follow_redirection(false);

    assert!(session.get("example.test/").unwrap());
    assert_eq!(session.result_code(), 302);
}

#[test]
fn max_redirect_limits_the_hops() {
    let stub = StubTransport::new(vec![
        response(302, &[("Location", "/a")], b""),
        response(302, &[("Location", "/b")], b""),
    ]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_max_redirect(1);

    assert!(!session.get("example.test/").unwrap());
    assert_eq!(stub.requests().len(), 2);
}

#[test]
fn compress_off_drops_accept_encoding_on_reset() {
    let stub = StubTransport::new(vec![response(200, &[], b"ok")]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let mut session = client.session();
    session.set_compress(false);
    session.reset();

    session.get("example.test/").unwrap();

    assert!(
        !stub.requests()[0]
            .headers
            .iter()
            .any(|(n, _)| n == "Accept-Encoding")
    );
}

#[test]
fn parse_server_cookies_stores_the_response_cookies_in_the_jar() {
    let stub = StubTransport::new(vec![response(
        200,
        &[("Set-Cookie", "sid=1; path=/")],
        b"ok",
    )]);
    let client = HttpClient::with_transport(stub.clone()).unwrap();
    let module = client.module("m");
    let mut session = client.session_for(&module);
    session.set_enabled_cookies(false);
    session.get("example.test/").unwrap();
    assert_eq!(
        module.cookies().get_server_cookies("example.test", "sid"),
        ""
    );

    session.parse_server_cookies();

    assert_eq!(
        module.cookies().get_server_cookies("example.test", "sid"),
        "sid=1; domain=example.test; path=/"
    );
}
