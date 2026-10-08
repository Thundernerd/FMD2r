//! `fmd.crypto` known-answer vectors, run as Lua snippets on the public runtime
//! (docs/tickets/T11-fmd-crypto.md, "Seams under test").

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

use fmd_lua::Runtime;

/// Runs `chunk` with `c` bound to `require 'fmd.crypto'`.
fn run(chunk: &str) {
    let runtime = Runtime::new().unwrap();
    let code = format!("local c = require 'fmd.crypto'\n{chunk}");
    if let Err(e) = runtime.exec(&code) {
        panic!("{e}");
    }
}

#[test]
fn base64_round_trips_binary() {
    run(r"
        assert(c.EncodeBase64('a\0b') == 'YQBi')
        assert(c.DecodeBase64('YQBi') == 'a\0b')
    ");
}

#[test]
fn url_encoding_follows_synapse_special_sets() {
    // EncodeURLElement escapes URLSpecialChar + URLFullSpecialChar, EncodeURL only the former
    // (baseunits/synapse/synacode.pas:91-94, 540-550); hex digits are upper case (IntToHex).
    run(r"
        assert(c.EncodeURLElement('a b&c') == 'a%20b%26c')
        assert(c.EncodeURLElement(';/?:@=&#+') == '%3B%2F%3F%3A%40%3D%26%23%2B')
        assert(c.EncodeURLElement('-_.!~*()') == '-_.!~*()')
        assert(c.EncodeURL('a b&c/?') == 'a%20b&c/?')
        assert(c.EncodeURL('\0\127\255<>%') == '%00%7F%FF%3C%3E%25')
    ");
}

#[test]
fn url_decoding_keeps_synapse_quirks() {
    // DecodeTriplet (baseunits/synapse/synacode.pas:403-476): a bad escape is kept verbatim,
    // and a '%' in the last two bytes ends decoding.
    run(r"
        assert(c.DecodeURL('a%20b%2fc%2F') == 'a b/c/')
        assert(c.DecodeURL('%zz1') == '%zz1')
        assert(c.DecodeURL('%4x') == '%4x')
        assert(c.DecodeURL('a%4') == 'a')
        assert(c.DecodeURL('100%') == '100')
        assert(c.DecodeURL('%00%ff') == '\0\255')
    ");
}

#[test]
fn html_encode_escapes_like_fpc_escape_html() {
    // EscapeHTML (FPC packages/fcl-xml/src/htmlelements.pp:146) also escapes the apostrophe.
    run(r#"
        assert(c.HTMLEncode('<a&"b">') == '&lt;a&amp;&quot;b&quot;&gt;')
        assert(c.HTMLEncode("it's\0") == 'it&#39;s\0')
    "#);
}

#[test]
fn html_decode_follows_fmd2_entity_rules() {
    // HTMLDecode (baseunits/uBaseUnit.pas:1312-1380): only amp/lt/gt/nbsp/quot and decimal
    // `&#N;` (read with Val, so `&#x41;` is 0 and `&#321;` wraps to a byte).
    run(r#"
        assert(c.HTMLDecode('&lt;a&amp;&quot;b&quot;&gt;&nbsp;') == '<a&"b"> ')
        assert(c.HTMLDecode('&#65;&#$42;&#x41;&#321;') == 'AB\0A')
        assert(c.HTMLDecode('caf\195\169 &#39;x&#39;') == 'caf\195\169 \'x\'')
        assert(c.HTMLDecode('a\0b') == 'a')
    "#);
}

#[test]
fn hex_helpers_match_basecrypto_case() {
    // HexToStr / StrToHexStr (baseunits/BaseCrypto.pas:56-63, 98-102); BinToHex writes upper case.
    run(r"
        assert(c.HexToStr('414243') == 'ABC')
        assert(c.HexToStr('00ff7F4') == '\0\255\127')
        assert(c.StrToHexStr('\0\171\255') == '00ABFF')
        assert(not pcall(c.HexToStr, 'zz'))
    ");
}

#[test]
fn digests_return_raw_bytes_and_hex_variants_lower_case() {
    // FIPS 180 / RFC 1321 / RFC 1320 "abc" vectors.
    run(r"
        assert(c.SHA256('abc') == c.HexToStr('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'))
        assert(c.SHA256Hex('abc') == 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')
        assert(c.SHA512('abc') == c.HexToStr('ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f'))
        assert(c.SHA512Hex('abc') == 'ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f')
        assert(c.SHA1('abc') == c.HexToStr('a9993e364706816aba3e25717850c26c9cd0d89d'))
        assert(c.SHA1Hex('abc') == 'a9993e364706816aba3e25717850c26c9cd0d89d')
        assert(c.MD5('abc') == c.HexToStr('900150983cd24fb0d6963f7d28e17f72'))
        assert(c.MD5Hex('abc') == '900150983cd24fb0d6963f7d28e17f72')
        assert(c.MD4('abc') == c.HexToStr('a448017aaf21d8525fc10ae87aa6729d'))
        assert(c.MD5Hex('') == 'd41d8cd98f00b204e9800998ecf8427e')
        assert(c.SHA256('a\0b') == c.HexToStr('59b271ae1bbcb1d31d41929817f4b16fb439eb4f31520b5ad1d5ce98920a7138'))
    ");
    // SHA1Hex returns '' for empty input (baseunits/BaseCrypto.pas:311).
    run("assert(c.SHA1Hex('') == '')");
}

#[test]
fn long_hashes_hash_the_value_repeated_to_len_bytes() {
    // MD5LongHash / SHA1LongHash (baseunits/synapse/synacode.pas:1142-1160, 1370-1388).
    run(r"
        -- digests of 'ababa', 'xyzxyzx' and '' from md5sum/sha1sum
        assert(c.MD5LongHash('ab', 5) == c.HexToStr('88cbc990f555585c848f265d56bbb85a'))
        assert(c.SHA1LongHash('xyz', 7) == c.HexToStr('22b36d9633bfa098fed09405add2c535b4eb22ad'))
        assert(c.MD5LongHash('ab', 0) == c.HexToStr('d41d8cd98f00b204e9800998ecf8427e'))
    ");
}

#[test]
fn hmacs_take_data_then_key() {
    // RFC 4231 / RFC 2202 test case 2 ("Jefe").
    run(r"
        local d = 'what do ya want for nothing?'
        assert(c.HMAC_SHA256(d, 'Jefe') == c.HexToStr('5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843'))
        assert(c.HMAC_SHA256Hex(d, 'Jefe') == '5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843')
        assert(c.HMAC_SHA512(d, 'Jefe') == c.HexToStr('164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737'))
        assert(c.HMAC_SHA512Hex(d, 'Jefe') == '164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737')
        assert(c.HMAC_MD5(d, 'Jefe') == c.HexToStr('750c783e6ab0b503eaa86e310a5db738'))
        assert(c.HMAC_SHA1(d, 'Jefe') == c.HexToStr('effcdf6ae5eb2fa2d27416d5f184df9c259a7c79'))
        assert(c.HMAC_SHA1Hex(d, 'Jefe') == 'effcdf6ae5eb2fa2d27416d5f184df9c259a7c79')
    ");
}

#[test]
fn hmacs_hash_long_keys_like_fmd2() {
    // RFC 4231 / RFC 2202 test case 6 (key longer than the block).
    run(r"
        local d = 'Test Using Larger Than Block-Size Key - Hash Key First'
        assert(c.HMAC_SHA256Hex(d, string.rep('\170', 131)) == '60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54')
        assert(c.HMAC_SHA1(d, string.rep('\170', 80)) == c.HexToStr('aa4ae5e15272d00e95705637ce8a3b55ed402112'))
        -- HMAC_SHA1Hex keys with the *hex text* of SHA1(key) (baseunits/BaseCrypto.pas:336-337):
        -- HMAC-SHA1 keyed with '4ca0ef38f1794b28a8f8ee110ee79d48ce13be25', computed with openssl.
        assert(c.HMAC_SHA1Hex(d, string.rep('\170', 80)) == '7607bbee543ed6df192bf23373d212fe1a7c111e')
    ");
}

#[test]
fn basecrypto_hmacs_return_empty_for_empty_data_or_key() {
    // baseunits/BaseCrypto.pas:334, 409, 455, 490, 536.
    run(r"
        for _, f in ipairs({'HMAC_SHA1Hex', 'HMAC_SHA256', 'HMAC_SHA256Hex', 'HMAC_SHA512', 'HMAC_SHA512Hex'}) do
            assert(c[f]('', 'k') == '', f)
            assert(c[f]('d', '') == '', f)
        end
        assert(c.HMAC_MD5('', '') == c.HexToStr('74e6f7298a9c2d168935f58c001bad88'))
    ");
}

#[test]
fn crcs_match_synapse_tables() {
    // Crc32 is CRC-32/ISO-HDLC; Crc16 is the reflected 0x8408 table with init $FFFF and no
    // final xor (baseunits/synapse/synacode.pas:309-342, 838-866). Check value of '123456789'.
    run(r"
        assert(c.CRC32('123456789') == 0xCBF43926)
        assert(c.CRC16('123456789') == 0x6F91)
        assert(c.CRC32('') == 0)
        assert(c.CRC16('') == 0xFFFF)
    ");
}

#[test]
fn uu_coding_uses_backtick_table_without_padding() {
    // EncodeUU/DecodeUU (baseunits/synapse/synacode.pas:734-772): a length symbol then the
    // groups, with the padding symbol dropped; inputs of 64+ bytes encode to ''.
    run(r#"
        assert(c.EncodeUU('Cat') == '#0V%T')
        assert(c.EncodeUU('hello world\0') == ',:&5L;&\\@=V]R;&0`')
        assert(c.EncodeUU('ab') == '"86(')
        assert(c.EncodeUU(string.rep('x', 64)) == '')
        assert(c.DecodeUU('#0V%T') == 'Cat')
        assert(c.DecodeUU(',:&5L;&\\@=V]R;&0`') == 'hello world\0')
        assert(c.DecodeUU('"86(') == 'ab')
        assert(c.DecodeUU('begin 644 x') == '')
        assert(c.DecodeUU('') == '')
    "#);
}

#[test]
fn base64url_swaps_alphabet_and_drops_padding() {
    // baseunits/BaseCrypto.pas:782-801.
    run(r"
        assert(c.EncodeBase64URL('\251\255\191\0') == '-_-_AA')
        assert(c.DecodeBase64URL('-_-_AA') == '\251\255\191\0')
        assert(c.DecodeBase64URL('YQ') == 'a')
    ");
}

/// SP 800-38A's AES-128 key and the IV 00 01 .. 0f.
const AES_PRELUDE: &str = r"
    local K = c.HexToStr('2b7e151628aed2a6abf7158809cf4f3c')
    local IV = c.HexToStr('000102030405060708090a0b0c0d0e0f')
    local P1 = c.HexToStr('6bc1bee22e409f96e93d7e117393172a')
    local P21 = '0123456789abcdef\0wxyz'
";

fn run_aes(chunk: &str) {
    run(&format!("{AES_PRELUDE}{chunk}"));
}

#[test]
fn aes_cbc_matches_sp800_38a_and_dcpcrypt_tail_handling() {
    // SP 800-38A F.2.1. DCPcrypt encrypts a partial last block by xoring it with E(C_last)
    // (dcpblockciphers.pas EncryptCBC); `cbc21` was computed with OpenSSL that way.
    run_aes(
        r"
        assert(c.AESEncryptCBC(P1, K, IV) == c.HexToStr('7649abac8119b246cee98e9b12e9197d'))
        assert(c.AESDecryptCBC(c.HexToStr('7649abac8119b246cee98e9b12e9197d'), K, IV) == P1)
        local cbc21 = c.HexToStr('64768548007aef9f3d258e5c34cdc21bb148e661c7')
        assert(c.AESEncryptCBC(P21, K, IV) == cbc21)
        assert(c.AESDecryptCBC(cbc21, K, IV) == P21)
    ",
    );
}

#[test]
fn aes_cbc_zero_pads_short_keys_and_ivs() {
    // Init(keyBytes[0], Length(key) * 8, ...) with DCPcrypt's Rijndael zero-padding the key to
    // 16/24/32 bytes; the IV is the first 16 bytes, zero-padded (baseunits/BaseCrypto.pas:175-205).
    run_aes(
        r"
        local P16 = P21:sub(1, 16)
        assert(c.AESEncryptCBC(P16, 'k', IV) == c.HexToStr('7575bf8b7d1833993ffc1e61af970c72'))
        assert(c.AESEncryptCBC(P16, 'k\0', IV .. 'ignored') == c.HexToStr('7575bf8b7d1833993ffc1e61af970c72'))
        assert(c.AESEncryptCBC(P16, '0123456789abcdefg', IV) == c.HexToStr('30b3a1937bdadf45d0cecdd586c26d5f'))
        assert(c.AESEncryptCBC(P16, K, '\0') == c.AESEncryptCBC(P16, K, string.rep('\0', 16)))
        -- empty arguments and keys over 256 bits give '' (the Pascal swallows the exception)
        assert(c.AESEncryptCBC('', K, IV) == '')
        assert(c.AESEncryptCBC(P16, '', IV) == '')
        assert(c.AESDecryptCBC(P16, K, '') == '')
        assert(c.AESEncryptCBC(P16, string.rep('k', 33), IV) == '')
    ",
    );
}

#[test]
fn aes_stream_modes_match_sp800_38a_and_openssl() {
    // SP 800-38A F.5.1 (CTR), F.3.13 (CFB128), F.4.1 (OFB); 21-byte vectors from OpenSSL.
    // AESCTR increments the whole 128-bit counter (baseunits/BaseCrypto.pas:634-643).
    run_aes(
        r"
        local CTR0 = c.HexToStr('f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff')
        assert(c.AESCTR(P1, K, CTR0) == c.HexToStr('874d6191b620e3261bef6864990db6ce'))
        local ctr = c.HexToStr('bac3b43276c2b0c331091d785c1acfca7d80137560')
        assert(c.AESCTR(P21, K, string.rep('\255', 16)) == ctr)
        assert(c.AESCTR(ctr, K, string.rep('\255', 16)) == P21)
        assert(c.AESCFB(P1, K, IV) == c.HexToStr('3b3fd92eb72dad20333449f8e83cfb4a'))
        assert(c.AESCFB(P21, K, IV) == c.HexToStr('60cf55ffad580481e230568bf8cb8906218f70a0ad'))
        assert(c.AESOFB(P1, K, IV) == c.HexToStr('3b3fd92eb72dad20333449f8e83cfb4a'))
        local ofb = c.HexToStr('60cf55ffad580481e230568bf8cb8906d9d3a2a372')
        assert(c.AESOFB(P21, K, IV) == ofb)
        assert(c.AESOFB(ofb, K, IV) == P21)
        for _, f in ipairs({'AESCTR', 'AESCFB', 'AESOFB'}) do
            assert(c[f]('', K, IV) == '' and c[f](P1, '', IV) == '' and c[f](P1, K, '') == '', f)
        end
    ",
    );
}

#[test]
fn aes_ecb_pkcs7_pads_and_unpads() {
    // SP 800-38A F.1.1 plus PKCS#7 (baseunits/BaseCrypto.pas:104-119, 541-596); padded
    // vectors from OpenSSL.
    run_aes(
        r"
        assert(c.AESEncryptECBPkcs7(P1, K):sub(1, 16) == c.HexToStr('3ad77bb40d7a3660a89ecaf32466ef97'))
        local abc = c.HexToStr('0da7d34a2c0c32bd408e96dbd66f3ffe')
        assert(c.AESEncryptECBPkcs7('abc', K) == abc)
        assert(c.AESDecryptECBPkcs7(abc, K) == 'abc')
        local e21 = c.HexToStr('5d9caf02529ee002dcff2b13ff1a8f70524c8f1ff18ef4ed7ffb74f591db478b')
        assert(c.AESEncryptECBPkcs7(P21, K) == e21)
        assert(c.AESDecryptECBPkcs7(e21, K) == P21)
        assert(c.AESEncryptECBPkcs7('', K) == '' and c.AESDecryptECBPkcs7(abc, '') == '')
    ",
    );
}

#[test]
fn aes_cbc_sha256_base64_pkcs7_hashes_the_key_and_takes_a_hex_iv() {
    // InitStr(key, TDCP_sha256), SetIV(HexToBytes(iv)), PKCS#7, Base64
    // (baseunits/BaseCrypto.pas:121-173); vector from OpenSSL with key SHA256('secret').
    run(r"
        local iv = '000102030405060708090a0b0c0d0e0f'
        assert(c.AESEncryptCBCSHA256Base64Pkcs7('hello world', 'secret', iv) == 'YCFl7lY0iP2cMukhb0V4lw==')
        assert(c.AESDecryptCBCSHA256Base64Pkcs7('YCFl7lY0iP2cMukhb0V4lw==', 'secret', iv) == 'hello world')
        assert(c.AESDecryptCBCSHA256Base64Pkcs7('YCFl7lY0iP2cMukhb0V4lw==', 'secret', '0') == '')
        assert(c.AESDecryptCBCSHA256Base64Pkcs7('YCFl7lY0iP2cMukhb0V4lw==', 'secret', 'zz') == '')
        assert(c.AESEncryptCBCSHA256Base64Pkcs7('', 'secret', iv) == '')
    ");
}

#[test]
fn aes_cbc_base64_zero_padding_helpers() {
    // MD5 variant: key is the 32-character MD5Hex text (AES-256), the IV raw, and despite its
    // name nothing is stripped (baseunits/BaseCrypto.pas:239-244). Hex variant: hex key and
    // IV, trailing NULs stripped (:246-273). Ciphertext from OpenSSL.
    run(r"
        local ct = 'y9+h7Rp9cRBy6hPATVkDSQ=='
        assert(c.AESDecryptCBCMD5Base64ZerosPadding(ct, 'pass', 'iv') == 'zero padded\0\0\0\0\0')
        local hexkey = c.StrToHexStr(c.MD5Hex('pass'))
        assert(c.AESDecryptCBCHexBase64ZerosPadding(ct, hexkey, '69760000000000000000000000000000') == 'zero padded')
        assert(c.AESDecryptCBCMD5Base64ZerosPadding('', 'pass', 'iv') == '')
    ");
}

#[test]
fn aes_gcm_appends_the_tag_and_rejects_tampering() {
    // McGrew & Viega GCM test case 2, then OpenSSL vectors with AAD and with 8- and 16-byte
    // IVs, which go through GHASH (baseunits/BaseCrypto.pas:928-1063).
    run_aes(
        r"
        local Z = string.rep('\0', 16)
        local tc2 = c.HexToStr('0388dace60b6a392f328c2b971b2fe78ab6e47d42cec13bdf53a67b21257bddf')
        assert(c.AESEncryptGCM(Z, Z, Z:sub(1, 12)) == tc2)
        assert(c.AESDecryptGCM(tc2, Z, Z:sub(1, 12)) == Z)
        local aad = 'header\0bytes-aad-xyz'
        local g12 = c.HexToStr('6bfe09658c5db875d8e175e4c1edd1f10b4d798f3709419e06cb98bf4b4a5d1db43f26be04')
        assert(c.AESEncryptGCM(P21, K, IV:sub(1, 12), aad) == g12)
        assert(c.AESDecryptGCM(g12, K, IV:sub(1, 12), aad) == P21)
        assert(c.AESDecryptGCM(g12, K, IV:sub(1, 12)) == '')
        assert(c.AESDecryptGCM(g12:sub(1, -2) .. 'x', K, IV:sub(1, 12), aad) == '')
        local g8 = c.HexToStr('cccfe2de2e8d33fd5ac0104612bcb0b27913d226873166c2d530af89b95de30711cb53f584')
        assert(c.AESEncryptGCM(P21, K, IV:sub(1, 8)) == g8)
        assert(c.AESDecryptGCM(g8, K, IV:sub(1, 8)) == P21)
        local g16 = c.HexToStr('7d2c0fa0bf8ad2c8ac6450f885a9017767f48e9d2132b8dd09843bff8cd6aaaf')
        assert(c.AESEncryptGCM(P21:sub(1, 16), K, IV, 'hea') == g16)
        assert(c.AESEncryptGCM('', K, IV) == '' and c.AESDecryptGCM(Z:sub(1, 15), K, IV) == '')
    ",
    );
}

#[test]
fn rc4_matches_known_vectors() {
    // Classic RC4 vectors (Key/Plaintext, Wiki/pedia, Secret/Attack at dawn);
    // baseunits/BaseCrypto.pas:290-304.
    run(r"
        assert(c.RC4('Plaintext', 'Key') == c.HexToStr('bbf316e8d940af0ad3'))
        assert(c.RC4('pedia', 'Wiki') == c.HexToStr('1021bf0420'))
        assert(c.RC4(c.HexToStr('45a01f645fc35b383552544b9bf5'), 'Secret') == 'Attack at dawn')
        assert(c.RC4('', 'Key') == '' and c.RC4('x', '') == '')
    ");
}

#[test]
fn pbkdf2_sha256_matches_known_vector() {
    // PBKDF2-HMAC-SHA256('password', 'salt', 4096) (dkLen 32 is the RFC 7914 §11-style
    // vector; 40 bytes from OpenSSL); baseunits/BaseCrypto.pas:746-780.
    run(r"
        local dk = c.HexToStr('c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134af7ad98c1b458ce3f')
        assert(c.PBKDF2SHA256('password', 'salt', 4096, 40) == dk)
        assert(c.PBKDF2SHA256('password', 'salt', 4096, 32) == dk:sub(1, 32))
        assert(c.PBKDF2SHA256('password', 'salt', 0, 32) == '')
        assert(c.PBKDF2SHA256('', 'salt', 1, 32) == '')
    ");
}

// EncryptString/DecryptString (baseunits/uBaseUnit.pas:1556-1590): DCPcrypt Rijndael keyed by
// InitStr(EncryptKey, TDCP_sha512) (first 256 bits of the SHA-512), IV E(0^16), 8-bit CFB,
// Base64. Ciphertexts reproduced with OpenSSL (aes-256-cfb8) from that recipe.

#[test]
fn encrypt_string_matches_fmd2_account_passwords() {
    run(r"
        assert(c.EncryptString('hunter2') == 'pcjhSQpguA==')
        assert(c.DecryptString('pcjhSQpguA==') == 'hunter2')
        assert(c.EncryptString('hunter2\0pw') == 'pcjhSQpguMa9wQ==')
        assert(c.DecryptString('pcjhSQpguMa9wQ==') == 'hunter2\0pw')
        assert(c.EncryptString('') == '' and c.DecryptString('') == '')
    ");
}

#[test]
fn encrypt_string_is_public_rust_api() {
    use fmd_lua::crypto::{decrypt_string, encrypt_string};
    assert_eq!(decrypt_string(b"pcjhSQpguA=="), b"hunter2");
    assert_eq!(encrypt_string(b"hunter2"), b"pcjhSQpguA==");
    let secret = b"p\0ss\xffw0rd with a longer tail than one block";
    assert_eq!(decrypt_string(&encrypt_string(secret)), secret);
}

#[test]
fn x25519_matches_rfc7748() {
    // RFC 7748 §6.1; wrong key lengths and an all-zero shared secret (OpenSSL's derive fails)
    // give '' (baseunits/BaseCrypto.pas:1380-1434).
    run(r"
        local a = c.HexToStr('77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a')
        local b = c.HexToStr('5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb')
        local A = c.HexToStr('8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a')
        local B = c.HexToStr('de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f')
        local K = c.HexToStr('4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742')
        assert(c.X25519_PublicKey(a) == A)
        assert(c.X25519_PublicKey(b) == B)
        assert(c.X25519_SharedSecret(a, B) == K)
        assert(c.X25519_SharedSecret(b, A) == K)
        assert(c.X25519_PublicKey(a:sub(2)) == '')
        assert(c.X25519_SharedSecret(a, B:sub(2)) == '')
        assert(c.X25519_SharedSecret(a, string.rep('\0', 32)) == '')
    ");
}

#[test]
fn secretstream_pulls_libsodium_push_output() {
    // Header and chunks from libsodium's crypto_secretstream_xchacha20poly1305_push: messages
    // 'hello' (MESSAGE), 32 bytes (PUSH), 'rekey\0me' (REKEY), 20 bytes (MESSAGE, after the
    // rekey), '' (FINAL) (baseunits/BaseCrypto.pas:1436-1534).
    run(r"
        local key = c.HexToStr('01080f001d242b323940474e555c636a71787f868d949ba2a9b0b7bec5ccd3da')
        local header = c.HexToStr('ae486c3f9ef94a04d2058b683dcaa8997c3e091a9a811fef')
        local chunks = {
            {'54cd4b5b940976c4bbedead2753b142316b1ffe6dbf8', 'hello', 0},
            {'3ae395d7e9e486c55f43adc066216e86ecefee75b7cbdd5e1446cbcf7fef8d0df822f306864ea8e930ee7736a848534eb2',
             '0123456789abcdef0123456789ABCDEF', 1},
            {'d30772928badf36764d900858e9fa27ddd9cc09ad584db2dc0', 'rekey\0me', 2},
            {'89f3e978ba4c8b23f33ad75cf8aacb04698c7f46f5a1cbd9737c72c39cbb4db90eced17958',
             'after the rekey!\0\1\2\3', 0},
            {'1dbc4dbe2fbe25942955244cad9c57ea9d', '', 3},
        }
        local state = c.SecretStream_InitPull(header, key)
        assert(#state == 64)
        local first = state
        for i, ch in ipairs(chunks) do
            local msg, tag
            state, msg, tag = c.SecretStream_Pull(state, c.HexToStr(ch[1]))
            assert(state, i)
            assert(msg == ch[2], i)
            assert(tag == ch[3], i)
        end
        -- a tampered chunk, a replayed chunk and a short chunk return nothing
        local bad = c.HexToStr(chunks[1][1]):sub(1, -2) .. 'x'
        assert(select('#', c.SecretStream_Pull(first, bad)) == 0)
        assert(c.SecretStream_Pull(state, c.HexToStr(chunks[1][1])) == nil)
        assert(c.SecretStream_Pull(first, string.rep('\0', 16)) == nil)
        -- wrong header or key lengths give nil
        assert(c.SecretStream_InitPull(header:sub(2), key) == nil)
        assert(c.SecretStream_InitPull(header, key:sub(2)) == nil)
    ");
}

#[test]
fn ticket_seam_snippet() {
    run(r#"
        assert(c.EncodeBase64('a\0b') == 'YQBi')
        assert(c.DecodeBase64('YQBi') == 'a\0b')
        assert(c.EncodeURLElement('a b&c') == 'a%20b%26c')
        assert(c.HexToStr('414243') == 'ABC')
        assert(c.SHA256('abc') == c.HexToStr('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'))
        assert(c.HTMLEncode('<a&"b">') == '&lt;a&amp;&quot;b&quot;&gt;')
    "#);
}

#[test]
fn nul_bytes_in_keys_and_data_are_kept() {
    // HMAC-SHA256 keyed with 'k\0y' over 'a\0b', computed with OpenSSL.
    run(r"
        assert(c.HMAC_SHA256Hex('a\0b', 'k\0y') == '20b3a393e9f0e606ae91954f1f871ebd5cd9c3a8f26e7b22beaed26ab4891891')
        assert(c.HMAC_SHA256Hex('a\0b', 'k\0y') ~= c.HMAC_SHA256Hex('a', 'k'))
        assert(c.StrToHexStr(c.DecodeURL(c.EncodeURLElement('\0x\0'))) == '007800')
    ");
}

#[test]
fn every_fmd2_function_is_registered() {
    // The method tables of baseunits/lua/LuaCrypto.pas:391-451.
    run(r"
        local names = {
            'EncryptString', 'DecryptString', 'HTMLDecode', 'HTMLEncode', 'HexToStr', 'StrToHexStr',
            'MD5Hex', 'SHA1Hex', 'HMAC_SHA1Hex', 'SHA256', 'SHA256Hex', 'SHA512', 'SHA512Hex',
            'HMAC_SHA256', 'HMAC_SHA256Hex', 'HMAC_SHA512', 'HMAC_SHA512Hex', 'AESEncryptCBC',
            'AESDecryptCBC', 'AESEncryptCBCSHA256Base64Pkcs7', 'AESDecryptCBCSHA256Base64Pkcs7',
            'AESDecryptCBCMD5Base64ZerosPadding', 'AESDecryptCBCHexBase64ZerosPadding',
            'AESEncryptECBPkcs7', 'AESDecryptECBPkcs7', 'AESCTR', 'AESCFB', 'AESOFB', 'RC4',
            'PBKDF2SHA256', 'EncodeBase64URL', 'DecodeBase64URL', 'AESEncryptGCM', 'AESDecryptGCM',
            'X25519_PublicKey', 'X25519_SharedSecret', 'SecretStream_InitPull', 'SecretStream_Pull',
            'DecodeURL', 'EncodeURL', 'DecodeUU', 'EncodeUU', 'EncodeURLElement', 'DecodeBase64',
            'EncodeBase64', 'CRC16', 'CRC32', 'MD4', 'MD5', 'HMAC_MD5', 'MD5LongHash', 'SHA1',
            'HMAC_SHA1', 'SHA1LongHash',
        }
        local n = 0
        for _, name in ipairs(names) do
            assert(type(c[name]) == 'function', name)
            n = n + 1
        end
        local registered = 0
        for _ in pairs(c) do registered = registered + 1 end
        assert(n == 54 and registered == n, registered)
    ");
}

#[test]
fn hex_base64_helper_raises_on_bad_hex() {
    // HexToBytes(key/iv) runs before the try/except (baseunits/BaseCrypto.pas:254-255), so
    // its EConvertError reaches Lua.
    run(r"
        local ct = 'y9+h7Rp9cRBy6hPATVkDSQ=='
        assert(not pcall(c.AESDecryptCBCHexBase64ZerosPadding, ct, 'zz', '00'))
        assert(not pcall(c.AESDecryptCBCHexBase64ZerosPadding, ct, '00', 'zz'))
    ");
}

#[test]
fn integer_arguments_wrap_to_pascal_integer() {
    // iterations/dkLen/Len are 32-bit `Integer` parameters fed from lua_tointeger
    // (baseunits/lua/LuaCrypto.pas:208-212, 286-290), so they keep the low 32 bits.
    run(r"
        local dk = c.PBKDF2SHA256('password', 'salt', 1, 32)
        assert(c.PBKDF2SHA256('password', 'salt', 1, (1 << 32) + 32) == dk)
        assert(c.PBKDF2SHA256('password', 'salt', 1, 1000000000000) == '')
        assert(c.MD5LongHash('ab', (1 << 32) + 5) == c.MD5('ababa'))
    ");
}

#[test]
fn gcm_rejects_an_explicit_nil_aad() {
    // With 4+ arguments the AAD is read with luaL_checklstring (baseunits/lua/LuaCrypto.pas:304-315).
    run_aes(
        r"
        assert(not pcall(c.AESEncryptGCM, P21, K, IV:sub(1, 12), nil))
        assert(not pcall(c.AESDecryptGCM, P21, K, IV:sub(1, 12), nil))
    ",
    );
}
