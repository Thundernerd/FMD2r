//! `require 'fmd.crypto'`: FMD2's hashing, encoding and cipher library
//! (baseunits/lua/LuaCrypto.pas), byte-for-byte compatible with the Pascal it ports.
//!
//! Every function is binary-safe: arguments are read with their explicit length and results
//! pushed with theirs (`GetLuaString`/`PushLuaString`, baseunits/lua/LuaCrypto.pas:15-31).
//! Digests return raw bytes; the `*Hex` variants return lower-case hex, while `StrToHexStr`
//! returns upper case like FPC's `BinToHex`.
//!
//! [`encrypt_string`] and [`decrypt_string`] are also public Rust API, for reading the
//! account passwords FMD2 stores with them.
//!
//! # Coverage
//!
//! All 54 functions of FMD2's method tables (baseunits/lua/LuaCrypto.pas:391-451) are
//! implemented; none is missing.
//!
//! | Tier | Functions |
//! |------|-----------|
//! | 1 | `HTMLEncode`, `DecodeBase64`, `EncodeURLElement`, `EncodeBase64`, `SHA256`, `MD5`, `HexToStr`, `HMAC_SHA256`, `DecodeBase64URL`, `SHA512`, `SHA1`, `DecodeURL`, `EncodeURL`, `RC4`, `AESCTR`, `HTMLDecode`, `HMAC_SHA256Hex`, `AESDecryptGCM`, `AESDecryptCBCSHA256Base64Pkcs7`, `EncodeBase64URL` |
//! | 2 | `X25519_PublicKey`, `X25519_SharedSecret`, `SecretStream_InitPull`, `SecretStream_Pull` |
//! | 3 | `StrToHexStr`, `MD5Hex`, `SHA1Hex`, `SHA256Hex`, `SHA512Hex`, `HMAC_SHA1`, `HMAC_SHA1Hex`, `HMAC_SHA512`, `HMAC_SHA512Hex`, `HMAC_MD5`, `MD4`, `MD5LongHash`, `SHA1LongHash`, `CRC16`, `CRC32`, `EncodeUU`, `DecodeUU`, `AESEncryptCBC`, `AESDecryptCBC`, `AESEncryptECBPkcs7`, `AESDecryptECBPkcs7`, `AESCFB`, `AESOFB`, `AESEncryptGCM`, `AESEncryptCBCSHA256Base64Pkcs7`, `AESDecryptCBCMD5Base64ZerosPadding`, `AESDecryptCBCHexBase64ZerosPadding`, `PBKDF2SHA256`, `EncryptString`, `DecryptString` |
//!
//! # Deliberate differences
//!
//! Where the Pascal's result is undefined (uninitialised memory, reads past a buffer) a
//! defined result is chosen; each case is documented on the function:
//! `AESEncryptECBPkcs7`/`AESDecryptECBPkcs7` process every block (the Pascal only the first),
//! `HTMLDecode` copies undecodable entities through, and short hex IVs are zero-padded.
//! A Pascal exception that escapes to Lua (bad hex in `HexToStr`, an invalid GCM or RC4 key
//! size, `*LongHash` of '') is a Lua error naming the function.

mod base;
mod dcp;
mod gcm;
mod html;
mod sodium;
mod synacode;

use mlua::{FromLua, FromLuaMulti, IntoLuaMulti, Lua, Table};

/// A failure the Pascal reports by raising an exception, surfaced as a Lua error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `HexToStr` met a pair that is not two hex digits (`StrToInt` raises EConvertError).
    #[error("invalid hex pair {0:?}")]
    InvalidHex(String),
    /// `MD5LongHash`/`SHA1LongHash` of an empty value (the Pascal divides by its length).
    #[error("division by zero")]
    DivisionByZero,
    /// A cipher key that DCPcrypt's `Init` rejects (empty or over 256 bits).
    #[error("invalid key size")]
    InvalidKeySize,
}

/// The passphrase FMD2 encrypts stored account passwords with (baseunits/uBaseUnit.pas:1556-1557).
const ENCRYPT_KEY: &[u8] = b"B74945FB50E84FD58BF9FEAB8E4BEA6B";

/// DCPcrypt's Rijndael after `InitStr(EncryptKey, TDCP_sha512)`: the key is the first 256 bits
/// (Rijndael's maximum) of SHA-512(EncryptKey), the IV the default E(0^16)
/// (dcpcrypt2.pas:430-457).
fn encrypt_key_cipher() -> Option<dcp::Rijndael> {
    dcp::Rijndael::new(&base::sha512(ENCRYPT_KEY)[..32], None)
}

/// FMD2's `EncryptString`: 8-bit CFB under the fixed `EncryptKey`, then Base64; '' stays ''
/// (baseunits/uBaseUnit.pas:1559-1573, dcpcrypt2.pas:661-666). FMD2 stores account passwords
/// this way.
pub fn encrypt_string(s: &[u8]) -> Vec<u8> {
    match encrypt_key_cipher() {
        Some(mut r) if !s.is_empty() => synacode::encode_base64(&r.encrypt_cfb8(s)),
        _ => Vec::new(),
    }
}

/// FMD2's `DecryptString`, the inverse of [`encrypt_string`]
/// (baseunits/uBaseUnit.pas:1575-1589, dcpcrypt2.pas:669-673).
pub fn decrypt_string(s: &[u8]) -> Vec<u8> {
    match encrypt_key_cipher() {
        Some(mut r) if !s.is_empty() => r.decrypt_cfb8(&synacode::decode_base64(s)),
        _ => Vec::new(),
    }
}

/// Upper-case hex digits, as FPC's `IntToHex` and `BinToHex` write them.
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// The value of one hex digit, either case.
fn hex_value(c: u8) -> Option<u8> {
    char::from(c).to_digit(16).map(|d| d as u8)
}

/// Compares two MACs without an early exit, as the Pascal ors the byte differences
/// (baseunits/BaseCrypto.pas:1033-1036, 1514-1518).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

/// A Pascal exception that escapes a crypto function, as a Lua error naming the function.
fn lua_error(name: &str, e: Error) -> mlua::Error {
    mlua::Error::runtime(format!("{name}: {e}"))
}

/// An integer argument as the Pascal receives it: `lua_tointeger` (0 for anything that is not
/// an integral number) passed to a 32-bit `Integer` parameter, which keeps the low 32 bits.
fn pascal_integer(lua: &Lua, v: mlua::Value) -> i32 {
    lua.coerce_integer(v).ok().flatten().unwrap_or(0) as i32
}

/// A Lua string argument, read as raw bytes like `GetLuaString`
/// (baseunits/lua/LuaCrypto.pas:15-22).
type Bytes = mlua::LuaString;

/// A `(s, key, iv)` cipher function.
type Fn3 = fn(&[u8], &[u8], &[u8]) -> Vec<u8>;
/// An AES-GCM function: `(s, key, iv, aad)`.
type GcmFn = fn(&[u8], &[u8], &[u8], &[u8]) -> Result<Vec<u8>, Error>;

/// Registers a one-argument byte-string function.
fn add1(lua: &Lua, t: &Table, name: &str, f: fn(&[u8]) -> Vec<u8>) -> mlua::Result<()> {
    let func = lua.create_function(move |lua, a: Bytes| lua.create_string(f(&a.as_bytes())))?;
    t.set(name, func)
}

/// Registers a two-argument byte-string function.
fn add2(lua: &Lua, t: &Table, name: &str, f: fn(&[u8], &[u8]) -> Vec<u8>) -> mlua::Result<()> {
    let func = lua.create_function(move |lua, (a, b): (Bytes, Bytes)| {
        lua.create_string(f(&a.as_bytes(), &b.as_bytes()))
    })?;
    t.set(name, func)
}

/// Registers a three-argument byte-string function.
fn add3(lua: &Lua, t: &Table, name: &str, f: Fn3) -> mlua::Result<()> {
    let func = lua.create_function(move |lua, (a, b, c): (Bytes, Bytes, Bytes)| {
        lua.create_string(f(&a.as_bytes(), &b.as_bytes(), &c.as_bytes()))
    })?;
    t.set(name, func)
}

/// Registers a one-argument byte-string function that may fail.
fn add1_try(
    lua: &Lua,
    t: &Table,
    name: &'static str,
    f: fn(&[u8]) -> Result<Vec<u8>, Error>,
) -> mlua::Result<()> {
    let func = lua.create_function(move |lua, a: Bytes| {
        lua.create_string(f(&a.as_bytes()).map_err(|e| lua_error(name, e))?)
    })?;
    t.set(name, func)
}

/// Registers a `(value, len)` long-hash function; `len` is read with `lua_tointeger`, so a
/// non-integer is 0 (baseunits/lua/LuaCrypto.pas:208-212, 226-230).
fn add_long_hash(
    lua: &Lua,
    t: &Table,
    name: &'static str,
    f: fn(&[u8], i32) -> Result<Vec<u8>, Error>,
) -> mlua::Result<()> {
    let func = lua.create_function(move |lua, (a, len): (Bytes, mlua::Value)| {
        let len = pascal_integer(lua, len);
        let out = f(&a.as_bytes(), len).map_err(|e| lua_error(name, e))?;
        lua.create_string(out)
    })?;
    t.set(name, func)
}

/// Registers an AES-GCM function: `(s, key, iv[, aad])`. Only a missing fourth argument
/// means an empty AAD; a fourth argument that is present (even `nil`) must be a string, as the
/// Pascal checks `lua_gettop(L) >= 4` (baseunits/lua/LuaCrypto.pas:304-328).
fn add_gcm(lua: &Lua, t: &Table, name: &'static str, f: GcmFn) -> mlua::Result<()> {
    let func = lua.create_function(move |lua, args: mlua::MultiValue| {
        let has_aad = args.len() >= 4;
        let (s, key, iv, aad): (Bytes, Bytes, Bytes, mlua::Value) =
            FromLuaMulti::from_lua_multi(args, lua)?;
        let aad = if has_aad {
            Bytes::from_lua(aad, lua)?.as_bytes().to_vec()
        } else {
            Vec::new()
        };
        let out = f(&s.as_bytes(), &key.as_bytes(), &iv.as_bytes(), &aad)
            .map_err(|e| lua_error(name, e))?;
        lua.create_string(out)
    })?;
    t.set(name, func)
}

/// Builds the library table, like `luaopen_crypto` (baseunits/lua/LuaCrypto.pas:453-457).
fn open(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    add1(lua, &t, "EncodeBase64", synacode::encode_base64)?;
    add1(lua, &t, "DecodeBase64", synacode::decode_base64)?;
    add1(lua, &t, "EncodeURLElement", synacode::encode_url_element)?;
    add1(lua, &t, "EncodeURL", synacode::encode_url)?;
    add1(lua, &t, "DecodeURL", synacode::decode_url)?;
    add1(lua, &t, "EncodeBase64URL", base::encode_base64_url)?;
    add1(lua, &t, "DecodeBase64URL", base::decode_base64_url)?;
    add1(lua, &t, "EncodeUU", synacode::encode_uu)?;
    add1(lua, &t, "DecodeUU", synacode::decode_uu)?;
    // Crc16/Crc32 are pushed as integers (baseunits/lua/LuaCrypto.pas:178-188).
    t.set(
        "CRC16",
        lua.create_function(|_, a: Bytes| Ok(synacode::crc16(&a.as_bytes())))?,
    )?;
    t.set(
        "CRC32",
        lua.create_function(|_, a: Bytes| Ok(synacode::crc32(&a.as_bytes())))?,
    )?;
    add3(lua, &t, "AESEncryptCBC", base::aes_encrypt_cbc)?;
    add3(lua, &t, "AESDecryptCBC", base::aes_decrypt_cbc)?;
    add3(lua, &t, "AESCTR", base::aes_ctr)?;
    add3(lua, &t, "AESCFB", base::aes_cfb)?;
    add3(lua, &t, "AESOFB", base::aes_ofb)?;
    add2(lua, &t, "AESEncryptECBPkcs7", base::aes_encrypt_ecb_pkcs7)?;
    add2(lua, &t, "AESDecryptECBPkcs7", base::aes_decrypt_ecb_pkcs7)?;
    add3(
        lua,
        &t,
        "AESEncryptCBCSHA256Base64Pkcs7",
        base::aes_encrypt_cbc_sha256_base64_pkcs7,
    )?;
    add3(
        lua,
        &t,
        "AESDecryptCBCSHA256Base64Pkcs7",
        base::aes_decrypt_cbc_sha256_base64_pkcs7,
    )?;
    add3(
        lua,
        &t,
        "AESDecryptCBCMD5Base64ZerosPadding",
        base::aes_decrypt_cbc_md5_base64_zeros_padding,
    )?;
    let hex_b64 = lua.create_function(|lua, (s, key, iv): (Bytes, Bytes, Bytes)| {
        let out = base::aes_decrypt_cbc_hex_base64_zeros_padding(
            &s.as_bytes(),
            &key.as_bytes(),
            &iv.as_bytes(),
        )
        .map_err(|e| lua_error("AESDecryptCBCHexBase64ZerosPadding", e))?;
        lua.create_string(out)
    })?;
    t.set("AESDecryptCBCHexBase64ZerosPadding", hex_b64)?;
    add_gcm(lua, &t, "AESEncryptGCM", gcm::encrypt)?;
    add_gcm(lua, &t, "AESDecryptGCM", gcm::decrypt)?;
    let rc4 = lua.create_function(|lua, (s, key): (Bytes, Bytes)| {
        let out = base::rc4(&s.as_bytes(), &key.as_bytes()).map_err(|e| lua_error("RC4", e))?;
        lua.create_string(out)
    })?;
    t.set("RC4", rc4)?;
    // Iterations and dkLen are read with `lua_tointeger` (baseunits/lua/LuaCrypto.pas:286-290).
    let pbkdf2 = lua.create_function(
        |lua, (p, s, it, len): (Bytes, Bytes, mlua::Value, mlua::Value)| {
            let (it, len) = (pascal_integer(lua, it), pascal_integer(lua, len));
            lua.create_string(base::pbkdf2_sha256(&p.as_bytes(), &s.as_bytes(), it, len))
        },
    )?;
    t.set("PBKDF2SHA256", pbkdf2)?;
    add1(lua, &t, "EncryptString", encrypt_string)?;
    add1(lua, &t, "DecryptString", decrypt_string)?;
    add1(lua, &t, "X25519_PublicKey", sodium::x25519_public_key)?;
    add2(lua, &t, "X25519_SharedSecret", sodium::x25519_shared_secret)?;
    // nil on failure (baseunits/lua/LuaCrypto.pas:342-351).
    let init_pull = lua.create_function(|lua, (header, key): (Bytes, Bytes)| {
        sodium::init_pull(&header.as_bytes(), &key.as_bytes())
            .map(|st| lua.create_string(st))
            .transpose()
    })?;
    t.set("SecretStream_InitPull", init_pull)?;
    // state, msg, tag on success, no values on failure (baseunits/lua/LuaCrypto.pas:353-371).
    let pull = lua.create_function(|lua, (state, chunk): (Bytes, Bytes)| {
        let Some(p) = sodium::pull(&state.as_bytes(), &chunk.as_bytes()) else {
            return Ok(mlua::MultiValue::new());
        };
        (
            lua.create_string(p.state)?,
            lua.create_string(p.msg)?,
            p.tag,
        )
            .into_lua_multi(lua)
    })?;
    t.set("SecretStream_Pull", pull)?;
    add1(lua, &t, "HTMLEncode", html::encode)?;
    add1(lua, &t, "HTMLDecode", html::decode)?;
    add1_try(lua, &t, "HexToStr", base::hex_to_str)?;
    add1(lua, &t, "StrToHexStr", base::str_to_hex_str)?;
    add1(lua, &t, "MD4", synacode::md4)?;
    add1(lua, &t, "MD5", synacode::md5)?;
    add1(lua, &t, "MD5Hex", base::md5_hex)?;
    add1(lua, &t, "SHA1", synacode::sha1)?;
    add1(lua, &t, "SHA1Hex", base::sha1_hex)?;
    add1(lua, &t, "SHA256", base::sha256)?;
    add1(lua, &t, "SHA256Hex", base::sha256_hex)?;
    add1(lua, &t, "SHA512", base::sha512)?;
    add1(lua, &t, "SHA512Hex", base::sha512_hex)?;
    add2(lua, &t, "HMAC_MD5", synacode::hmac_md5)?;
    add2(lua, &t, "HMAC_SHA1", synacode::hmac_sha1)?;
    add2(lua, &t, "HMAC_SHA1Hex", base::hmac_sha1_hex)?;
    add2(lua, &t, "HMAC_SHA256", base::hmac_sha256)?;
    add2(lua, &t, "HMAC_SHA256Hex", base::hmac_sha256_hex)?;
    add2(lua, &t, "HMAC_SHA512", base::hmac_sha512)?;
    add2(lua, &t, "HMAC_SHA512Hex", base::hmac_sha512_hex)?;
    add_long_hash(lua, &t, "MD5LongHash", synacode::md5_long_hash)?;
    add_long_hash(lua, &t, "SHA1LongHash", synacode::sha1_long_hash)?;
    Ok(t)
}

/// Makes `require 'fmd.crypto'` return the library, as `LuaPackage.AddLib('crypto', ...)`
/// does (baseunits/lua/LuaCrypto.pas:460, baseunits/lua/LuaPackage.pas:62-79).
pub(crate) fn register(lua: &Lua) -> mlua::Result<()> {
    crate::package::add_lib(lua, "crypto", open)
}
