# T11: `fmd.crypto`
Deps: T03

## Goal
Implement `require 'fmd.crypto'` with RustCrypto crates, byte-for-byte compatible with FMD2's `LuaCrypto.pas`/`BaseCrypto.pas`, prioritising the functions upstream modules actually call.

## Scope (in/out)
In, in this order (usage counts across upstream `lua/` in brackets, measured at plan time):
1. **Tier 1 (most used):** `HTMLEncode` [47], `DecodeBase64` [23], `EncodeURLElement` [20], `EncodeBase64` [8], `SHA256` [7], `MD5` [6], `HexToStr` [6], `HMAC_SHA256` [5], `DecodeBase64URL` [4], `SHA512`, `SHA1`, `DecodeURL`, `EncodeURL`, `RC4`, `AESCTR`, `HTMLDecode`, `HMAC_SHA256Hex`, `AESDecryptGCM`, `AESDecryptCBCSHA256Base64Pkcs7`, `EncodeBase64URL`.
2. **Tier 2:** `X25519_PublicKey`, `X25519_SharedSecret`, `SecretStream_InitPull`, `SecretStream_Pull` (libsodium-compatible `crypto_secretstream_xchacha20poly1305`; use a pure-Rust crate if one is wire-compatible, else `libsodium-sys`).
3. **Tier 3 (unused upstream today, but part of the API):** everything else in the method table: `StrToHexStr`, `MD5Hex`, `SHA1Hex`, `SHA256Hex`, `SHA512Hex`, `HMAC_SHA1`, `HMAC_SHA1Hex`, `HMAC_SHA512`, `HMAC_SHA512Hex`, `HMAC_MD5`, `MD4`, `MD5LongHash`, `SHA1LongHash`, `CRC16`, `CRC32`, `EncodeUU`, `DecodeUU`, `AESEncryptCBC`, `AESDecryptCBC`, `AESEncryptECBPkcs7`, `AESDecryptECBPkcs7`, `AESCFB`, `AESOFB`, `AESEncryptGCM`, `AESEncryptCBCSHA256Base64Pkcs7`, `AESDecryptCBCMD5Base64ZerosPadding`, `AESDecryptCBCHexBase64ZerosPadding`, `PBKDF2SHA256`, `EncryptString`, `DecryptString`.
- Binary-safe in and out (`GetLuaString`/`PushLuaString` use explicit lengths). Hash functions return raw bytes vs hex exactly as the Pascal does (check each: e.g. `MD5` vs `MD5Hex`).
- `EncryptString`/`DecryptString` must match FMD2 (key `EncryptKey`, DCPcrypt Rijndael initialised with `InitStr(key, TDCP_sha512)`); expose the same functions as a public Rust API because T31/T32 reuse them to decrypt FMD2 account passwords.
- A coverage list in the crate docs marking which functions are implemented.

Out: other libs.

## Seams under test
- Lua snippets through the `fmd-lua` runtime with known-answer vectors:
```lua
local c = require 'fmd.crypto'
assert(c.EncodeBase64('a\0b') == 'YQBi')
assert(c.DecodeBase64('YQBi') == 'a\0b')
assert(c.EncodeURLElement('a b&c') == 'a%20b%26c')   -- confirm exact set from synacode
assert(c.HexToStr('414243') == 'ABC')
assert(c.SHA256('abc') == c.HexToStr('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'))
assert(c.HTMLEncode('<a&"b">') == '&lt;a&amp;&quot;b&quot;&gt;')
```
- Vectors for AES modes, GCM, X25519 and SecretStream from RFC/NIST/libsodium test vectors; for FMD2-specific helpers (`AESDecryptCBCSHA256Base64Pkcs7`, `EncryptString`) derive vectors by reading `BaseCrypto.pas` and, where possible, from strings produced by FMD2 itself (e.g. an `accounts.db` row).
- Public Rust API `fmd_lua::crypto::{encrypt_string, decrypt_string}` (or wherever it lives) round-trips and decrypts a known FMD2 ciphertext.

## Acceptance criteria
- [ ] Tier 1 and Tier 2 complete with vectors; Tier 3 complete or explicitly listed as missing in the crate docs (a missing function raises a clear Lua error naming it).
- [ ] Every function binary-safe; NUL bytes in keys/IVs/data round-trip.
- [ ] Hex output case matches FMD2 (check `BytesToHex`).
- [ ] Doc comments cite `LuaCrypto.pas` and `BaseCrypto.pas` lines.

## FMD2 references
- `baseunits/lua/LuaCrypto.pas:15-31` (binary-safe string get/push), `:32-384` (each wrapper), `:385-458` (method table and `luaopen_crypto`)
- `baseunits/BaseCrypto.pas:10-52` (interface), `:56-120` (hex, Pkcs7), `:121-275` (AES CBC variants), `:275-405` (MD5/RC4/SHA/HMAC), and the remainder (GCM, X25519, SecretStream, PBKDF2)
- `baseunits/uBaseUnit.pas:1556-1590` (`EncryptKey`, `EncryptString`, `DecryptString`)
- `baseunits/synapse/synacode.pas` (`EncodeURLElement`, `EncodeURL`, `DecodeURL`, Base64, UU, CRC)
- `docs/LUA-REFERENCE.md:1111-1139` (`fmd.crypto` as modules use it)
