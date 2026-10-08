//! Ports of the Synapse `synacode` routines `fmd.crypto` exposes
//! (baseunits/synapse/synacode.pas).

use sha2::Digest;

use super::{Error, HEX_UPPER, hex_value};

/// Synapse's Base64 alphabet, with `=` as the 65th (padding) symbol
/// (baseunits/synapse/synacode.pas:95-96).
const TABLE_BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";

/// Encodes 3 bytes into 4 symbols of `table`, padding with its 65th symbol
/// (baseunits/synapse/synacode.pas:653-704).
fn encode3to4(value: &[u8], table: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len().div_ceil(3) * 4);
    for chunk in value.chunks(3) {
        let c0 = chunk[0];
        let mut d = [(c0 & 0xFC) >> 2, (c0 & 0x03) << 4, 0x40, 0x40];
        if let Some(&c1) = chunk.get(1) {
            d[1] += (c1 & 0xF0) >> 4;
            d[2] = (c1 & 0x0F) << 2;
            if let Some(&c2) = chunk.get(2) {
                d[2] += (c2 & 0xC0) >> 6;
                d[3] = c2 & 0x3F;
            }
        }
        // A symbol past the end of the table (UU has no padding symbol) is dropped (:695-700).
        out.extend(d.iter().filter_map(|&i| table.get(usize::from(i)).copied()));
    }
    out
}

/// `EncodeBase64` (baseunits/synapse/synacode.pas:713-716).
pub fn encode_base64(value: &[u8]) -> Vec<u8> {
    encode3to4(value, TABLE_BASE64)
}

/// The value of a Base64 symbol, or `None` for anything else, which the decoder skips
/// (`ReTablebase64`, baseunits/synapse/synacode.pas:102-111).
fn base64_value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// `DecodeBase64`: a lenient decoder that skips every non-alphabet byte (padding,
/// whitespace, garbage) and decodes a trailing group of 2 or 3 symbols to 1 or 2 bytes
/// (baseunits/synapse/synacode.pas:595-651, 706-709).
pub fn decode_base64(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut d: u32 = 0;
    let mut dl = 4;
    for c in value.iter().filter_map(|&c| base64_value(c)) {
        d = (d << 6) | c;
        dl -= 1;
        if dl == 0 {
            out.extend_from_slice(&d.to_be_bytes()[1..]);
            d = 0;
            dl = 4;
        }
    }
    match dl {
        1 => out.extend_from_slice(&(d >> 2).to_be_bytes()[2..]),
        2 => out.push((d >> 4) as u8),
        _ => {}
    }
    out
}

/// `URLSpecialChar` (baseunits/synapse/synacode.pas:93-94).
fn is_url_special(c: u8) -> bool {
    matches!(
        c,
        0x00..=0x20
            | b'<'
            | b'>'
            | b'"'
            | b'%'
            | b'{'
            | b'}'
            | b'|'
            | b'\\'
            | b'^'
            | b'['
            | b']'
            | b'`'
            | 0x7F..=0xFF
    )
}

/// `URLFullSpecialChar` (baseunits/synapse/synacode.pas:91-92).
fn is_url_full_special(c: u8) -> bool {
    matches!(
        c,
        b';' | b'/' | b'?' | b':' | b'@' | b'=' | b'&' | b'#' | b'+'
    )
}

/// Replaces every byte in `specials` by `%XX` (`EncodeTriplet`,
/// baseunits/synapse/synacode.pas:492-524).
fn encode_triplet(value: &[u8], specials: impl Fn(u8) -> bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() * 3);
    for &c in value {
        if specials(c) {
            out.extend_from_slice(&[
                b'%',
                HEX_UPPER[usize::from(c >> 4)],
                HEX_UPPER[usize::from(c & 15)],
            ]);
        } else {
            out.push(c);
        }
    }
    out
}

/// `EncodeURLElement` (baseunits/synapse/synacode.pas:540-543).
pub fn encode_url_element(value: &[u8]) -> Vec<u8> {
    encode_triplet(value, |c| is_url_special(c) || is_url_full_special(c))
}

/// `EncodeURL` (baseunits/synapse/synacode.pas:547-550).
pub fn encode_url(value: &[u8]) -> Vec<u8> {
    encode_triplet(value, is_url_special)
}

/// `DecodeURL` = `DecodeTriplet(Value, '%')` (baseunits/synapse/synacode.pas:403-476, 485-488):
/// a bad escape keeps the `%` and goes on with the next byte, `%` followed by CR/LF (a
/// soft line break) is dropped with it, and a `%` within the last two bytes ends decoding.
pub fn decode_url(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut x = 0;
    while x < value.len() {
        let c = value[x];
        x += 1;
        if c != b'%' {
            out.push(c);
            continue;
        }
        // Pascal's 1-based `x < lv` means at least two bytes follow the delimiter.
        if x + 1 >= value.len() {
            break;
        }
        match (value[x], value[x + 1]) {
            (b'\r', b'\n') | (b'\n', b'\r') => x += 2,
            (b'\r' | b'\n', _) => x += 1,
            (hi, lo) => match (hex_value(hi), hex_value(lo)) {
                (Some(hi), Some(lo)) => {
                    out.push((hi << 4) | lo);
                    x += 2;
                }
                _ => out.push(c),
            },
        }
    }
    out
}

/// `MD4` (baseunits/synapse/synacode.pas:1462).
pub fn md4(value: &[u8]) -> Vec<u8> {
    md4::Md4::digest(value).to_vec()
}

/// `MD5` (baseunits/synapse/synacode.pas:1104-1111).
pub fn md5(value: &[u8]) -> Vec<u8> {
    md5::Md5::digest(value).to_vec()
}

/// `SHA1` (baseunits/synapse/synacode.pas:1332-1339).
pub fn sha1(value: &[u8]) -> Vec<u8> {
    sha1::Sha1::digest(value).to_vec()
}

/// Hashes `value` repeated to `len` bytes, the body of `MD5LongHash` and `SHA1LongHash`
/// (baseunits/synapse/synacode.pas:1142-1160, 1370-1388). An empty `value` is an error
/// (the Pascal divides by its length); `len <= 0` hashes nothing.
fn long_hash<D: Digest>(value: &[u8], len: i32) -> Result<Vec<u8>, Error> {
    if value.is_empty() {
        return Err(Error::DivisionByZero);
    }
    let len = usize::try_from(len).unwrap_or(0);
    let mut d = D::new();
    for _ in 0..len / value.len() {
        d.update(value);
    }
    d.update(&value[..len % value.len()]);
    Ok(d.finalize().to_vec())
}

/// `MD5LongHash` (baseunits/synapse/synacode.pas:1142-1160).
pub fn md5_long_hash(value: &[u8], len: i32) -> Result<Vec<u8>, Error> {
    long_hash::<md5::Md5>(value, len)
}

/// `SHA1LongHash` (baseunits/synapse/synacode.pas:1370-1388).
pub fn sha1_long_hash(value: &[u8], len: i32) -> Result<Vec<u8>, Error> {
    long_hash::<sha1::Sha1>(value, len)
}

/// `HMAC_MD5(Text, Key)`, standard HMAC (baseunits/synapse/synacode.pas:1115-1140).
pub fn hmac_md5(text: &[u8], key: &[u8]) -> Vec<u8> {
    super::base::hmac::<md5::Md5>(text, key, 64, md5)
}

/// `HMAC_SHA1(Text, Key)`, standard HMAC (baseunits/synapse/synacode.pas:1343-1368).
pub fn hmac_sha1(text: &[u8], key: &[u8]) -> Vec<u8> {
    super::base::hmac::<sha1::Sha1>(text, key, 64, sha1)
}

/// Synapse's UU alphabet, with the backtick for 0 (baseunits/synapse/synacode.pas:99-100).
const TABLE_UU: &[u8] = b"`!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_";

/// Decodes groups of 4 symbols of `table` into 3 bytes; a symbol not in the table counts as
/// 0 and a short last group yields fewer bytes (baseunits/synapse/synacode.pas:554-593).
fn decode4to3(value: &[u8], table: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    for group in value.chunks(4) {
        let mut d = [64u8; 4];
        for (n, &c) in group.iter().enumerate() {
            d[n] = table.iter().position(|&t| t == c).unwrap_or(0) as u8;
        }
        out.push(((d[0] & 0x3F) << 2) + ((d[1] & 0x30) >> 4));
        if d[2] != 64 {
            out.push(((d[1] & 0x0F) << 4) + ((d[2] & 0x3C) >> 2));
            if d[3] != 64 {
                out.push(((d[2] & 0x03) << 6) + (d[3] & 0x3F));
            }
        }
    }
    out
}

/// `EncodeUU`: one UU line body — the length symbol, then the groups without padding — or ''
/// for input of 64 bytes or more (baseunits/synapse/synacode.pas:767-772).
pub fn encode_uu(value: &[u8]) -> Vec<u8> {
    match TABLE_UU.get(value.len()) {
        Some(&len) => {
            let mut out = vec![len];
            out.extend(encode3to4(value, TABLE_UU));
            out
        }
        None => Vec::new(),
    }
}

/// `DecodeUU`: decodes one UU line; `begin`/`end`/`table` lines and blank input give ''
/// (baseunits/synapse/synacode.pas:734-765).
pub fn decode_uu(value: &[u8]) -> Vec<u8> {
    // `Trim` strips bytes <= ' ' from both ends.
    let start = value.iter().position(|&c| c > b' ');
    let end = value.iter().rposition(|&c| c > b' ');
    let (Some(start), Some(end)) = (start, end) else {
        return Vec::new();
    };
    let s = value[start..=end].to_ascii_uppercase();
    if s.starts_with(b"BEGIN") || s.starts_with(b"END") || s.starts_with(b"TABLE") {
        return Vec::new();
    }
    // The length symbol is the untrimmed first byte; one not in the table ends decoding.
    let Some(n) = TABLE_UU.iter().position(|&t| t == value[0]) else {
        return Vec::new();
    };
    let x = match n % 3 {
        0 => n / 3 * 4,
        1 => n / 3 * 4 + 2,
        _ => n / 3 * 4 + 3,
    };
    let body = &value[1..value.len().min(1 + x)];
    if body.is_empty() {
        return Vec::new();
    }
    let mut s = body.to_vec();
    s.resize(x, b' ');
    decode4to3(&s, TABLE_UU)
}

/// A reflected CRC table: `Crc32Tab` is polynomial 0xEDB88320
/// (baseunits/synapse/synacode.pas:242-307), `Crc16Tab` 0x8408 (:309-342).
const fn reflected_table(poly: u32) -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ poly } else { c >> 1 };
            bit += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

const CRC32_TAB: [u32; 256] = reflected_table(0xEDB8_8320);
const CRC16_TAB: [u32; 256] = reflected_table(0x8408);

/// `Crc32` (baseunits/synapse/synacode.pas:830-846).
pub fn crc32(value: &[u8]) -> u32 {
    !value.iter().fold(0xFFFF_FFFFu32, |crc, &b| {
        (crc >> 8) ^ CRC32_TAB[usize::from(b ^ crc as u8)]
    })
}

/// `Crc16`: init $FFFF, no final xor (baseunits/synapse/synacode.pas:850-866).
pub fn crc16(value: &[u8]) -> u16 {
    value.iter().fold(0xFFFFu16, |crc, &b| {
        (crc >> 8) ^ CRC16_TAB[usize::from(b ^ crc as u8)] as u16
    })
}
