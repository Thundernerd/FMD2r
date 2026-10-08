//! Content-Encoding decoding, as done after the request loop in `DefaultHTTPRequest`.

use std::io::Read;

/// Decodes `body` by its `Content-Encoding` (baseunits/httpsendthread.pas:681-710): a value
/// containing `zstd` wins over `br`, which wins over `gzip`/`deflate`. A body that fails
/// to decode is returned unchanged, as FMD2 swallows the decoder's exception.
pub(crate) fn decode(content_encoding: &str, body: Vec<u8>) -> Vec<u8> {
    let encoding = content_encoding.to_lowercase();
    let decoded = if encoding.contains("zstd") {
        zstd::decode_all(body.as_slice()).ok()
    } else if encoding.contains("br") {
        read_all(brotli_decompressor::Decompressor::new(
            body.as_slice(),
            4096,
        ))
    } else if encoding.contains("gzip") || encoding.contains("deflate") {
        unzip(&body)
    } else {
        return body;
    };
    decoded.unwrap_or(body)
}

/// `unzipStream` (baseunits/GZIPUtils.pas:158): gzip when the gzip magic is present,
/// zlib otherwise. Raw deflate is accepted too, as servers send it for `deflate`.
fn unzip(body: &[u8]) -> Option<Vec<u8>> {
    if body.starts_with(&[0x1f, 0x8b]) {
        read_all(flate2::read::MultiGzDecoder::new(body))
    } else {
        read_all(flate2::read::ZlibDecoder::new(body))
            .or_else(|| read_all(flate2::read::DeflateDecoder::new(body)))
    }
}

fn read_all(mut reader: impl Read) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    reader.read_to_end(&mut out).ok()?;
    Some(out)
}
