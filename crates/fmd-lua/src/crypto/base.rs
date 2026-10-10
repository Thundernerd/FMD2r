//! Ports of FMD2's `BaseCrypto` unit (baseunits/BaseCrypto.pas).

use sha2::Digest;

use super::dcp::{Block, Rijndael};
use super::{Error, HEX_UPPER, hex_value};
// FPC's `EncodeStringBase64`/`DecodeStringBase64` are standard padded Base64; decoding
// reuses Synapse's lenient decoder, which agrees on every valid input.
use super::synacode::{decode_base64, encode_base64};

/// `HexToStr`: each pair of hex digits becomes one byte; a trailing odd digit is ignored, and
/// a non-hex pair is an error (`StrToInt` raises) (baseunits/BaseCrypto.pas:56-63).
pub fn hex_to_str(h: &[u8]) -> Result<Vec<u8>, Error> {
    h.chunks_exact(2)
        .map(|p| match (hex_value(p[0]), hex_value(p[1])) {
            (Some(hi), Some(lo)) => Ok((hi << 4) | lo),
            _ => Err(Error::InvalidHex(String::from_utf8_lossy(p).into_owned())),
        })
        .collect()
}

/// `StrToHexStr`: upper-case hex, as FPC's `BinToHex` writes it (baseunits/BaseCrypto.pas:98-102).
pub fn str_to_hex_str(s: &[u8]) -> Vec<u8> {
    s.iter()
        .flat_map(|&b| {
            [
                HEX_UPPER[usize::from(b >> 4)],
                HEX_UPPER[usize::from(b & 15)],
            ]
        })
        .collect()
}

/// `LowerCase(StrToHexStr(...))`, the form every `*Hex` digest returns
/// (e.g. baseunits/BaseCrypto.pas:287).
pub fn lower_hex(s: &[u8]) -> Vec<u8> {
    str_to_hex_str(s).to_ascii_lowercase()
}

/// `SHA256` (baseunits/BaseCrypto.pas:379-392).
pub fn sha256(s: &[u8]) -> Vec<u8> {
    sha2::Sha256::digest(s).to_vec()
}

/// `SHA256Hex` (baseunits/BaseCrypto.pas:394-397).
pub fn sha256_hex(s: &[u8]) -> Vec<u8> {
    lower_hex(&sha256(s))
}

/// `SHA512` (baseunits/BaseCrypto.pas:460-473).
pub fn sha512(s: &[u8]) -> Vec<u8> {
    sha2::Sha512::digest(s).to_vec()
}

/// `SHA512Hex` (baseunits/BaseCrypto.pas:475-478).
pub fn sha512_hex(s: &[u8]) -> Vec<u8> {
    lower_hex(&sha512(s))
}

/// `MD5Hex` (baseunits/BaseCrypto.pas:275-288).
pub fn md5_hex(s: &[u8]) -> Vec<u8> {
    lower_hex(&md5::Md5::digest(s))
}

/// `SHA1Hex`: empty input gives an empty string, not the digest of nothing
/// (baseunits/BaseCrypto.pas:306-321).
pub fn sha1_hex(s: &[u8]) -> Vec<u8> {
    if s.is_empty() {
        return Vec::new();
    }
    lower_hex(&sha1::Sha1::digest(s))
}

/// HMAC as both units write it out by hand: a key longer than the block is replaced by
/// `long_key(key)`, then `H(opad || H(ipad || s))` (baseunits/BaseCrypto.pas:323-377,
/// baseunits/synapse/synacode.pas:1115-1140).
pub(super) fn hmac<D: Digest>(
    s: &[u8],
    key: &[u8],
    block: usize,
    long_key: fn(&[u8]) -> Vec<u8>,
) -> Vec<u8> {
    let k = if key.len() > block {
        long_key(key)
    } else {
        key.to_vec()
    };
    let mut ipad = vec![0x36u8; block];
    let mut opad = vec![0x5Cu8; block];
    for (i, &b) in k.iter().take(block).enumerate() {
        ipad[i] ^= b;
        opad[i] ^= b;
    }
    let inner = D::new().chain_update(&ipad).chain_update(s).finalize();
    D::new()
        .chain_update(&opad)
        .chain_update(inner)
        .finalize()
        .to_vec()
}

/// `HMAC_SHA1Hex`: empty data or key gives ''; a key over 64 bytes is replaced by the
/// lower-case *hex text* of its SHA-1, not the raw digest (baseunits/BaseCrypto.pas:323-377).
pub fn hmac_sha1_hex(s: &[u8], key: &[u8]) -> Vec<u8> {
    if s.is_empty() || key.is_empty() {
        return Vec::new();
    }
    lower_hex(&hmac::<sha1::Sha1>(s, key, 64, sha1_hex))
}

/// `HMAC_SHA256`: empty data or key gives '' (baseunits/BaseCrypto.pas:399-451).
pub fn hmac_sha256(s: &[u8], key: &[u8]) -> Vec<u8> {
    if s.is_empty() || key.is_empty() {
        return Vec::new();
    }
    hmac::<sha2::Sha256>(s, key, 64, sha256)
}

/// `HMAC_SHA256Hex` (baseunits/BaseCrypto.pas:453-458).
pub fn hmac_sha256_hex(s: &[u8], key: &[u8]) -> Vec<u8> {
    lower_hex(&hmac_sha256(s, key))
}

/// `HMAC_SHA512`: empty data or key gives '' (baseunits/BaseCrypto.pas:480-532).
pub fn hmac_sha512(s: &[u8], key: &[u8]) -> Vec<u8> {
    if s.is_empty() || key.is_empty() {
        return Vec::new();
    }
    hmac::<sha2::Sha512>(s, key, 128, sha512)
}

/// `HMAC_SHA512Hex` (baseunits/BaseCrypto.pas:534-539).
pub fn hmac_sha512_hex(s: &[u8], key: &[u8]) -> Vec<u8> {
    lower_hex(&hmac_sha512(s, key))
}

/// `EncodeBase64URL`: Base64 with `-_` for `+/` and no padding (baseunits/BaseCrypto.pas:782-788).
pub fn encode_base64_url(s: &[u8]) -> Vec<u8> {
    encode_base64(s)
        .into_iter()
        .filter(|&c| c != b'=')
        .map(|c| match c {
            b'+' => b'-',
            b'/' => b'_',
            c => c,
        })
        .collect()
}

/// `DecodeBase64URL`: maps `-_` back to `+/` and decodes, padding implied
/// (baseunits/BaseCrypto.pas:790-801).
pub fn decode_base64_url(s: &[u8]) -> Vec<u8> {
    let std: Vec<u8> = s
        .iter()
        .map(|&c| match c {
            b'-' => b'+',
            b'_' => b'/',
            c => c,
        })
        .collect();
    decode_base64(&std)
}

/// `AESEncryptCBC(s, key, iv)`: raw key and IV, no padding; any empty argument or an invalid
/// key size gives '' (baseunits/BaseCrypto.pas:175-205).
pub fn aes_encrypt_cbc(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    if s.is_empty() || iv.is_empty() {
        return Vec::new();
    }
    Rijndael::new(key, Some(iv)).map_or_else(Vec::new, |mut r| r.encrypt_cbc(s))
}

/// `AESDecryptCBC(s, key, iv)` (baseunits/BaseCrypto.pas:207-237).
pub fn aes_decrypt_cbc(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    if s.is_empty() || iv.is_empty() {
        return Vec::new();
    }
    Rijndael::new(key, Some(iv)).map_or_else(Vec::new, |mut r| r.decrypt_cbc(s))
}

/// `Pkcs7AddPad`: always appends 1-16 bytes (baseunits/BaseCrypto.pas:104-113).
fn pkcs7_add_pad(s: &[u8]) -> Vec<u8> {
    let n = 16 - (s.len() & 15);
    let mut out = s.to_vec();
    out.resize(s.len() + n, n as u8);
    out
}

/// `Pkcs7RemovePad`: drops as many bytes as the last byte says, unchecked; more than the
/// whole string leaves '' (baseunits/BaseCrypto.pas:115-119).
fn pkcs7_remove_pad(mut s: Vec<u8>) -> Vec<u8> {
    let n = s.last().map_or(0, |&b| usize::from(b));
    s.truncate(s.len().saturating_sub(n));
    s
}

/// The keystream modes take the key raw with the nil-IV init and a separate 16-byte block
/// from `iv`, zero-padded (e.g. baseunits/BaseCrypto.pas:609-615).
fn keystream_setup(s: &[u8], key: &[u8], iv: &[u8]) -> Option<(Rijndael, Block)> {
    if s.is_empty() || iv.is_empty() {
        return None;
    }
    let r = Rijndael::new(key, None)?;
    let mut block = [0u8; 16];
    let n = iv.len().min(16);
    block[..n].copy_from_slice(&iv[..n]);
    Some((r, block))
}

/// `AESCTR(s, key, iv)`: the counter is the whole 16-byte block, incremented big-endian with
/// wrap-around (baseunits/BaseCrypto.pas:598-654).
pub fn aes_ctr(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    let Some((r, mut counter)) = keystream_setup(s, key, iv) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(s.len());
    for chunk in s.chunks(16) {
        let ks = r.encrypt_block(&counter);
        out.extend(chunk.iter().zip(ks).map(|(a, b)| a ^ b));
        counter = (u128::from_be_bytes(counter).wrapping_add(1)).to_be_bytes();
    }
    out
}

/// `AESCFB(s, key, iv)`: CFB-128, the ciphertext fed back (baseunits/BaseCrypto.pas:656-701).
/// The Pascal feeds back what it just wrote, so this one function only encrypts.
pub fn aes_cfb(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    let Some((r, mut feedback)) = keystream_setup(s, key, iv) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(s.len());
    for chunk in s.chunks(16) {
        let ks = r.encrypt_block(&feedback);
        for (j, &p) in chunk.iter().enumerate() {
            let c = p ^ ks[j];
            feedback[j] = c;
            out.push(c);
        }
    }
    out
}

/// `AESOFB(s, key, iv)`: OFB-128, its own inverse (baseunits/BaseCrypto.pas:703-744).
pub fn aes_ofb(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    let Some((r, mut output)) = keystream_setup(s, key, iv) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(s.len());
    for chunk in s.chunks(16) {
        output = r.encrypt_block(&output);
        out.extend(chunk.iter().zip(output).map(|(a, b)| a ^ b));
    }
    out
}

/// `AESEncryptECBPkcs7(s, key)` (baseunits/BaseCrypto.pas:541-568). The Pascal encrypts only
/// the first block, leaving the rest uninitialised; this encrypts every block, which agrees on
/// inputs under 16 bytes.
pub fn aes_encrypt_ecb_pkcs7(s: &[u8], key: &[u8]) -> Vec<u8> {
    if s.is_empty() {
        return Vec::new();
    }
    let Some(r) = Rijndael::new(key, None) else {
        return Vec::new();
    };
    pkcs7_add_pad(s)
        .chunks_exact(16)
        .flat_map(|b| r.encrypt_block(&to_block(b)))
        .collect()
}

/// `AESDecryptECBPkcs7(s, key)` (baseunits/BaseCrypto.pas:570-596). Decrypts every block where
/// the Pascal decrypts only the first; a partial block gives ''.
pub fn aes_decrypt_ecb_pkcs7(s: &[u8], key: &[u8]) -> Vec<u8> {
    if s.is_empty() || !s.len().is_multiple_of(16) {
        return Vec::new();
    }
    let Some(r) = Rijndael::new(key, None) else {
        return Vec::new();
    };
    pkcs7_remove_pad(
        s.chunks_exact(16)
            .flat_map(|b| r.decrypt_block(&to_block(b)))
            .collect(),
    )
}

fn to_block(b: &[u8]) -> Block {
    let mut out = [0u8; 16];
    out.copy_from_slice(b);
    out
}

/// `AESEncryptCBCSHA256Base64Pkcs7(s, key, iv)`: key SHA-256(key) via `InitStr`, IV from hex,
/// PKCS#7, then Base64; any empty argument or a bad hex IV gives ''
/// (baseunits/BaseCrypto.pas:121-146).
pub fn aes_encrypt_cbc_sha256_base64_pkcs7(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    match sha256_keyed(s, key, iv) {
        Some(mut r) => encode_base64(&r.encrypt_cbc(&pkcs7_add_pad(s))),
        None => Vec::new(),
    }
}

/// `AESDecryptCBCSHA256Base64Pkcs7(s, key, iv)`: the inverse, with the unchecked
/// `Pkcs7RemovePad` (baseunits/BaseCrypto.pas:148-173).
pub fn aes_decrypt_cbc_sha256_base64_pkcs7(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    let Some(mut r) = sha256_keyed(s, key, iv) else {
        return Vec::new();
    };
    // `Result[Length(Result)]` on an empty result faults, and the `except` returns ''.
    let data = decode_base64(s);
    if data.is_empty() {
        return Vec::new();
    }
    pkcs7_remove_pad(r.decrypt_cbc(&data))
}

/// `InitStr(key, TDCP_sha256)` then `SetIV(ivb[0])` from `HexToBytes(iv)`
/// (baseunits/BaseCrypto.pas:132-134, 159-161). An empty IV faults on `ivb[0]` (''); a short
/// one is zero-padded where the Pascal would read past it.
fn sha256_keyed(s: &[u8], key: &[u8], iv: &[u8]) -> Option<Rijndael> {
    if s.is_empty() || key.is_empty() || iv.is_empty() {
        return None;
    }
    let ivb = hex_to_str(iv).ok().filter(|b| !b.is_empty())?;
    let mut r = Rijndael::new(&sha256(key), None)?;
    r.set_iv(&ivb);
    Some(r)
}

/// `AESDecryptCBCMD5Base64ZerosPadding(s, key, iv)`: `AESDecryptCBC` of the Base64-decoded
/// data with the 32-character `MD5Hex(key)` as an AES-256 key. Nothing is stripped despite the
/// name (baseunits/BaseCrypto.pas:239-244).
pub fn aes_decrypt_cbc_md5_base64_zeros_padding(s: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    if s.is_empty() || key.is_empty() || iv.is_empty() {
        return Vec::new();
    }
    aes_decrypt_cbc(&decode_base64(s), &md5_hex(key), iv)
}

/// `AESDecryptCBCHexBase64ZerosPadding(s, key, iv)`: hex key and IV, Base64 data, trailing
/// NULs stripped (baseunits/BaseCrypto.pas:246-273). Bad hex is an error, as `HexToBytes` runs
/// before the `try` (:254-255); an invalid key size gives ''. An empty IV means DCPcrypt's
/// default; a short one is zero-padded where the Pascal would read past it.
pub fn aes_decrypt_cbc_hex_base64_zeros_padding(
    s: &[u8],
    key: &[u8],
    iv: &[u8],
) -> Result<Vec<u8>, Error> {
    if s.is_empty() || key.is_empty() || iv.is_empty() {
        return Ok(Vec::new());
    }
    let key = hex_to_str(key)?;
    let iv = hex_to_str(iv)?;
    let iv = Some(iv.as_slice()).filter(|b| !b.is_empty());
    let Some(mut r) = Rijndael::new(&key, iv) else {
        return Ok(Vec::new());
    };
    let mut out = r.decrypt_cbc(&decode_base64(s));
    while out.last() == Some(&0) {
        out.pop();
    }
    Ok(out)
}

/// `RC4(s, key)`: plain RC4 with no keystream drop; empty data or key gives ''
/// (baseunits/BaseCrypto.pas:290-304). A key over DCPcrypt's 2048-bit maximum escapes the
/// Pascal as an exception, here an error (dcpcrypt2.pas:417-425).
pub fn rc4(s: &[u8], key: &[u8]) -> Result<Vec<u8>, Error> {
    if s.is_empty() || key.is_empty() {
        return Ok(Vec::new());
    }
    if key.len() > 256 {
        return Err(Error::InvalidKeySize);
    }
    let mut state: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
        state.swap(i, usize::from(j));
    }
    let (mut i, mut j) = (0u8, 0u8);
    Ok(s.iter()
        .map(|&b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(state[usize::from(i)]);
            state.swap(usize::from(i), usize::from(j));
            b ^ state[usize::from(state[usize::from(i)].wrapping_add(state[usize::from(j)]))]
        })
        .collect())
}

/// `PBKDF2SHA256(password, salt, iterations, dkLen)`: standard PBKDF2-HMAC-SHA256; an empty
/// password or salt, or a count below 1, gives '' (baseunits/BaseCrypto.pas:746-780).
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: i32, dk_len: i32) -> Vec<u8> {
    if password.is_empty() || salt.is_empty() || iterations < 1 || dk_len < 1 {
        return Vec::new();
    }
    let Ok(dk_len) = usize::try_from(dk_len) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(dk_len.next_multiple_of(32));
    let mut block: u32 = 1;
    while out.len() < dk_len {
        let mut salted = salt.to_vec();
        salted.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac_sha256(&salted, password);
        let mut t = u.clone();
        for _ in 1..iterations {
            u = hmac_sha256(&u, password);
            t.iter_mut().zip(&u).for_each(|(a, b)| *a ^= b);
        }
        out.extend_from_slice(&t);
        block = block.wrapping_add(1);
    }
    out.truncate(dk_len);
    out
}
