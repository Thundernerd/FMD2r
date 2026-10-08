// Helpers outside #[test] fns are not covered by clippy.toml's test exemption; unwrap is fine in tests/.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::Write;

use axum::Router;
use axum::http::header;
use axum::routing::get;
use common::TestServer;
use fmd_http::HttpClient;

const TEXT: &[u8] = b"<html>chapter list</html>";

fn gzip() -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(TEXT).unwrap();
    e.finish().unwrap()
}

fn zlib() -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(TEXT).unwrap();
    e.finish().unwrap()
}

fn brotli() -> Vec<u8> {
    let mut out = Vec::new();
    brotli::BrotliCompress(&mut &TEXT[..], &mut out, &Default::default()).unwrap();
    out
}

fn zstd() -> Vec<u8> {
    zstd::encode_all(TEXT, 3).unwrap()
}

fn fetch(encoding: &'static str, body: Vec<u8>) -> Vec<u8> {
    let server = TestServer::start(Router::new().route(
        "/doc",
        get(move || async move { ([(header::CONTENT_ENCODING, encoding)], body) }),
    ));
    let client = HttpClient::new().unwrap();
    let mut session = client.session();
    assert!(session.get(&server.url("/doc")).unwrap());
    session.document().to_vec()
}

#[test]
fn gzip_bodies_are_decoded() {
    assert_eq!(fetch("gzip", gzip()), TEXT);
}

#[test]
fn deflate_bodies_are_decoded() {
    assert_eq!(fetch("deflate", zlib()), TEXT);
}

#[test]
fn brotli_bodies_are_decoded() {
    assert_eq!(fetch("br", brotli()), TEXT);
}

#[test]
fn zstd_bodies_are_decoded() {
    assert_eq!(fetch("zstd", zstd()), TEXT);
}

#[test]
fn a_body_that_fails_to_decode_is_kept_as_is() {
    assert_eq!(
        fetch("gzip", b"not gzip at all".to_vec()),
        b"not gzip at all"
    );
}
