# Native dependencies: upstream usage and Rust options for FMD2r

Research for [#5](https://github.com/Thundernerd/FMD2r/issues/5), part of map [#1](https://github.com/Thundernerd/FMD2r/issues/1).
Upstream baseline: FMD2 `ad3a5b63` (2026-10-04), checked out at `~/Repositories/Forks/FMD2`. Facts gathered 2026-10-08.
Counts are over everything under `lua/`: 624 modules, plus templates, `utils/` and `websitebypass/`.

## TL;DR

| Dependency | Upstream usage at baseline | Recommendation |
|---|---|---|
| `fmd.duktape` `ExecJS` | 13 call sites in 8 files. One of them is the `Madara` template, which 125 modules inherit. Bundled `crypto-js` is loaded through Duktape's CommonJS `require`. | **rquickjs (QuickJS-NG)**, plus a small CommonJS `require` shim and a fresh runtime per call. Fallback: vendor `duktape.c` 2.7 ourselves. |
| `fmd.pcre2` | **0 callers.** No file under `lua/` requires it. | Keep the API for drop-in compatibility. Back it with the `pcre2`/`pcre2-sys` crate (vendored C), so `gsub` can call `pcre2_substitute` directly. Do not use fancy-regex. |
| lua-protobuf (`require 'pb'`) | 1 module (MangaPlus), via `utils/protoc.lua` and `MangaPlus.proto`. | **Compile upstream `pb.c` with `cc` against mlua's vendored Lua**, then register `luaopen_pb` in `package.preload`. No Rust reimplementation is needed. |
| `fmd.crypto` (54 functions) | 20 functions are used. Hot ones: `HTMLEncode` (47 calls), `DecodeBase64` (23), `EncodeURLElement` (20). Rare ones: AES-CTR, AES-GCM, X25519 and libsodium secretstream (1 module each). | RustCrypto covers everything, but several functions have **non-standard quirks that must be replicated** (§4). `EncryptString` is AES-256-CFB8 with a fixed key and IV (§4.4). |
| `fmd.gzip` + HTTP content-encoding | `gzip.Inflate`: 1 module (Happymh). The HTTP client always sends `Accept-Encoding: gzip, deflate, br, zstd`. | `flate2` with gzip/zlib/raw **auto-detect**, `brotli-decompressor`, and `zstd` or `ruzstd`. Decode manually, mirroring FMD2's sniffing, rather than relying on reqwest's auto-decompression. |
| `fmd.imagepuzzle` | 7 modules. | Pure Rust with the `image` crate (webp/png/jpeg). Output is not byte-identical: it re-encodes to PNG or JPEG. |
| `fmd.mangafoxwatermark` | 1 module (FanFox). | Port the small Otsu + PSNR template matcher to Rust with `image`. |
| `fmd.fileutil` | 2 modules. | Trivial Rust, but `SerializeAndMaintainNames` needs a careful port. |
| `fmd.subprocess` | 2 callers. Both are **Windows-only as written**: `cmd.exe /c node …` (puppeteer) and `python lua\websitebypass\cloudflare.py`. | `std::process::Command`. See §8: these paths never work on Linux unmodified, which affects other tickets. |

All recommended C dependencies have permissive licences, so they are compatible with FMD2r's GPL-2.0: QuickJS-NG (MIT), PCRE2 (BSD), lua-protobuf (MIT) and zstd (BSD).

---

## 1. `fmd.duktape` — `ExecJS`

### 1.1 What FMD2 does

Sources: `baseunits/lua/LuaDuktape.pas` and `baseunits/Duktape.pas`.

- `ExecJS(src)` creates a **fresh Duktape heap per call** (`duk_create_heap_default`).
- It installs Duktape's CommonJS module loader (`duk_module_duktape_init`). It also installs a `Duktape.modSearch` that reads `lua/<id>`, falling back to `lua/<id>.js` (`loadModuleFile`, cached in a `TFileCache`).
- It adds a global `print` that joins its arguments with a space and logs them.
- It then runs `duk_peval` on the source as **global (sloppy) script code** and returns `duk_safe_to_string(completion value)`.
- `"undefined"` is mapped to `''`. A JS error raises a Pascal exception. The Lua binding catches that exception, logs it, and returns **no value**, so the result is `nil`.
- There is no timeout and no memory limit.
- Version: `Duktape.Api.pas` declares 2.3.0 headers, but the shipped `libduktape.dll` is **Duktape 2.7.0** (`dist/readme.md`).

### 1.2 Call sites (13 in 8 files)

| File | What runs | Origin of JS |
|---|---|---|
| `templates/Madara.lua:176` (125 modules use the template) | Site `chapter-protector-data` script, then `require("utils/crypto-js.min.js")` and `require("utils/cryptojs-aes-format.js")`, then `CryptoJS.AES.decrypt(chapter_data, nonce, {format: CryptoJSAesJson})` | site + bundled |
| `modules/MangaGo.lua:145,228` | A site JS fragment wrapped in a helper (`substr`/`substring`). Also crypto-js AES-CBC with `CryptoJS.pad.ZeroPadding` and a hex key/IV. | site + bundled |
| `modules/ReadComicOnline.lua:304` | About 200 lines of hand-written ES5. It polyfills `atob`, `endsWith` and `startsWith`, and uses regex `exec` loops and `decodeURIComponent`. | module |
| `modules/FanFox.lua:100,117` | Dean Edwards `eval(function(p,a,c,k,e,d)…)` packed script with a jQuery stub `$`. A second call evaluates the `chapterfun.ashx` response with `;d;` appended. | site (packed/eval) |
| `modules/acqqcom.lua:21,23,29` | A `window` stub, `p,a,c,k,e,r` packed script and `JSON.stringify` | site (packed/eval) |
| `modules/DigitalTeam.lua:33,42` | Page script with `JSON.stringify({m,ch,chs})`, then a whole POST response evaluated | site |
| `modules/ZeroScans.lua:60` | `window.__ZEROSCANS__` payload, then `JSON.stringify` | site |
| `websitebypass/cloudflare.lua:83` | Legacy IUAM v1 challenge with `document` stubs and `String.prototype.big/small/…` shims. The Cloudflare challenge it targets has been retired, so this path is dead in practice. | site |

Other packed JS is unpacked in Lua (`utils/jsunpack.lua`, 3 users) or `SoJsonV4Deobfuscator` in MangaGo, so no JS engine is involved there.

### 1.3 ES features the scripts rely on

- **Bundled scripts are ES5 only.**
  - `crypto-js.min.js` (48 KB, UMD) contains no `=>`, `let`, `const` or template literals.
  - `cryptojs-aes-format.js` sets `module.exports`.
  - ReadComicOnline polyfills `atob`, `endsWith` and `startsWith`, so module authors deliberately write to Duktape's limits.
- **Duktape-specific behaviour the scripts depend on:**
  1. **A CommonJS `require` with `module.exports`, resolving ids under `lua/`.** For example, `require("utils/crypto-js.min.js")` maps to `lua/utils/crypto-js.min.js`.
  2. **`require` must throw on an unknown id.** crypto-js does `try { crypto = require('crypto') } catch {}` and only needs secure randomness for *encrypt*, which no module calls.
  3. **Top-level `var` must create globals.** `cryptojs-aes-format.js` references the global `CryptoJS` that Madara defines with a top-level `var CryptoJS = require(...)`. The engine must therefore evaluate the source as a global script, not as an ES module or a function body.
  4. **The completion value of the script is the result.** Examples: `…;newImgs||guidkey;`, `…;DATA;` and `JSON.stringify(...)`.
  5. **The result is converted with plain `ToString`.** Objects come back as `[object Object]`, which is why every caller wraps results in `JSON.stringify`.
- Duktape 2.x is ES5.1 plus a partial set of ES2015+ features. Its post-ES5 status table lists `const` only as "mostly an alias for var", and does not list `let`, arrow functions, template literals or classes as supported ([duktape.org](https://duktape.org/), [Post-ES5 feature status](https://wiki.duktape.org/postes5features)). Any modern engine is therefore a **superset** for the syntax in use.

### 1.4 Rust options

| Option | Engine / maturity | ES level | Fit |
|---|---|---|---|
| **rquickjs 0.14.0** (updated 2026-09-18) | Binds **QuickJS-NG** (C, vendored by the crate). Its README calls it "feature complete, mostly stable" ([README](https://github.com/DelSkayn/rquickjs), [crates.io](https://crates.io/crates/rquickjs)). | ES2020+ | Fast and small. Has a `loader` feature for ES modules, but CommonJS needs a shim. |
| **boa_engine 0.21.0** | Pure Rust. Describes itself as "experimental" with ">90% of the latest spec" in test262 ([repo](https://github.com/boa-dev/boa)). | latest | No C, but its 0.x API churns and it is slower than QuickJS. No CommonJS. |
| Duktape crates | `duktape` 0.0.2 (2015), `duktape-rs` 0.0.4 (2019), `dukbind` 0.0.4 (2019), `dukt` 0.1.0 (2022). `kg-js` 0.10.1 (2026-08) is the only recently updated one ([crates.io search](https://crates.io/search?q=duktape)). | ES5.1 | Exact fidelity, but the bindings are effectively unmaintained. Vendoring `duktape.c` 2.7 with `cc` + bindgen ourselves is easy: one C file, and C is allowed. |

**Recommendation: rquickjs.** Reproduce Duktape's contract:

1. **Fresh `Runtime` + `Context` per `ExecJS` call**, as FMD2 does. Runtimes are cheap, and this sidesteps rquickjs's `!Send` types: create, use and drop the runtime on the Lua worker thread.
2. **Evaluate the source as a global script** (`ctx.eval::<Value, _>(src)`, which is global-script eval) and stringify the completion value with JS `String(v)`. Map `undefined` to `''`. On an exception, log it and return `nil`, not a Lua error.
3. **Install a global `require(id)`** implementing Duktape's CommonJS semantics:
   - Resolve the id against `lua/` (exact path first, then `id + ".js"`).
   - Wrap the source as `function(require, exports, module){…}`.
   - Return `module.exports`.
   - Cache per heap.
   - Throw `Error` when the module is not found.
   - Cache file contents across calls, as FMD2's `TFileCache` does.
4. **Install `print`**, routed to the module logger.
5. **Add what FMD2 lacks:** a `set_interrupt_handler` deadline (timeout) and a memory limit. Site JS is untrusted, and Duktape had no such guard.

Behavioural differences to watch, all edge cases:

- **Non-BMP output encoding.** Duktape strings are internally extended CESU-8, so astral characters come back as two 3-byte surrogate sequences. QuickJS-NG returns UTF-8. QuickJS is "more correct" here, and no module depends on CESU-8.
- **Top-level `const` redeclaration.** Duktape treats `const` like `var`, so `const x=1; var x=2;` works there but is a SyntaxError in QuickJS. No bundled script does this. Site scripts at baseline run under Duktape, so they do not use `let` or `const` in ways Duktape rejects.
- **Number→string formatting.** Both follow ECMAScript `Number::toString`, so no difference is expected.

Keep `ExecJS` behind a trait so that a vendored Duktape 2.7 can be swapped in if a divergence ever shows up. The trait also makes Duktape usable as a differential-testing oracle in CI.

---

## 2. `fmd.pcre2`

### 2.1 What FMD2 does

Source: `baseunits/lua/LuaPCRE2.pas`. It links the system `libpcre2-8`. The shipped DLL is PCRE2 10.47.

- `exec`, `find`, `match`, `gmatch` and `gsub` all take `(subject, pattern[, init])`. `gsub` takes `(subject, pattern, replacement[, init])`.
- Every pattern is compiled with **only `PCRE2_UTF`**: no `UCP`, no JIT, and no cache, so it recompiles on every call. A compile error is logged and the call returns `false`, nothing, or the original string.
- `find` returns 1-based `start, end`.
- `match` returns the whole match if there are no capture groups, otherwise captures 1..n-1. `n` is `pcre2_match`'s return value, so **unmatched trailing groups are omitted**.
- `gmatch` is an iterator that advances `StartOffset` to the end of the previous match. An **empty match therefore never advances**, which is an infinite-loop bug. Replicate it with a guard.
- `gsub` uses `pcre2_substitute` with `SUBSTITUTE_GLOBAL | SUBSTITUTE_OVERFLOW_LENGTH`. The **replacement syntax is PCRE2's** (`$1`, `${name}`), not Lua's `%1`. Text before `init` is copied unchanged.

### 2.2 Usage

**Zero.** No file under `lua/` mentions `pcre2`. It exists only as Host API surface.

### 2.3 Options

- **`pcre2` 0.2.11 / `pcre2-sys`** (BurntSushi) ([docs](https://docs.rs/pcre2/latest/pcre2/bytes/struct.RegexBuilder.html), [pcre2-sys README](https://github.com/BurntSushi/rust-pcre2/blob/master/pcre2-sys/README.md)):
  - Linking: it uses the system lib when available, otherwise builds bundled source statically. `PCRE2_SYS_STATIC=1` forces static linking. Bindings were generated against 10.42.
  - The high-level crate exposes `utf`/`ucp`/`jit` and the other builder flags, but **no substitute API**. Call `pcre2_substitute_8` through `pcre2-sys` directly.
  - This is the same C library FMD2 uses, so it gives exact semantics.
- **fancy-regex**: a backtracking layer over `regex`. It supports backreferences, lookaround, atomic groups and conditionals ([repo](https://github.com/fancy-regex/fancy-regex)). It lacks possessive quantifiers, `\K` and recursion, and has no PCRE2-style substitution. Its semantics differ, so it is not drop-in.
- **`regex`**: no backreferences or lookaround. Ruled out.

**Recommendation:** implement the five functions on `pcre2-sys` with static vendoring (`PCRE2_SYS_STATIC=1` in Docker). This is low effort and exact. It is low priority because nothing calls it yet.

---

## 3. lua-protobuf (`require 'pb'`)

### 3.1 What FMD2 does

FMD2 ships `pb.dll`, built from [starwing/lua-protobuf @ 75e3b51b](https://github.com/starwing/lua-protobuf/tree/75e3b51b8e8dda0343c9b4956e0c33b6eef44aa6) (`dist/readme.md`). It is a plain Lua C module found by `package.cpath`. No Pascal is involved.

### 3.2 Usage

There is one user, `modules/MangaPlus.lua`:

- `local protoc = require 'utils.protoc'` (the pure-Lua compiler from the same repo) and `local pb = require 'pb'`.
- `protoc.proto3_optional = true` and `protoc:load(ReadFile(target_file))`, which loads `MangaPlus.proto` (proto3, 95 lines).
- `pb.decode('Response', body)`, called 3 times.
- `protoc.lua` itself does `pcall(require, "pb")` and `pb.load(descriptor)`.

**Side finding:** MangaPlus finds its `.proto` with `debug.getinfo(1,'S').source` and strips `@`. FMD2r must therefore load module chunks with chunkname `"@<real file path>"`. This affects the Lua host ticket.

### 3.3 Options

- lua-protobuf is "a pair of C source: `pb.h` and `pb.c`" plus `protoc.lua`. It supports Lua 5.1–5.4 and is MIT-licensed ([repo](https://github.com/starwing/lua-protobuf)).
- mlua offers `unsafe fn create_c_function(lua_CFunction)`. Loading C modules requires `Lua::unsafe_new[_with]`, because safe mode refuses them ([docs](https://docs.rs/mlua/latest/mlua/struct.Lua.html)). FMD2r needs the unsafe/full stdlib anyway (#1: "Modules keep full stdlib").
- mlua-sys has `links = "lua"`, and the vendored `lua-src` prints `cargo:include=<dir>` ([lua-src](https://github.com/mlua-rs/lua-src-rs/blob/main/src/lib.rs), [mlua-sys Cargo.toml](https://github.com/mlua-rs/mlua/blob/main/mlua-sys/Cargo.toml)). A build script in a crate that depends on `mlua-sys` can read `DEP_LUA_INCLUDE` and compile `pb.c` against the exact same Lua headers.

**Recommendation:** vendor `pb.c`/`pb.h` at the pinned commit and compile them with `cc`. Then:

- Declare `extern "C" fn luaopen_pb(L) -> c_int`.
- Register it with `package.preload["pb"] = lua.create_c_function(luaopen_pb)`.

Everything links statically into one binary against one Lua 5.4, so there are no `pb.so` or ABI issues. A Rust reimplementation is unnecessary. Ship `utils/protoc.lua` unmodified, since it is upstream Lua.

---

## 4. `fmd.crypto`

### 4.1 Surface and usage

Sources:

- `baseunits/lua/LuaCrypto.pas`, which registers the 38 `cryptomethods` and 16 synapse `synacodemethods` in one table.
- `baseunits/BaseCrypto.pas`, built on DCPcrypt.
- `baseunits/uBaseUnit.pas`.
- `synapse/synacode.pas`.

The counts below include `require 'fmd.crypto'.X(...)` call forms. Of the 54 functions, 20 are used.

| Function | Calls / files | Example call site | Rust mapping |
|---|---|---|---|
| `HTMLEncode` | 47 / 29 | `CreateTXQuery(crypto.HTMLEncode(HTTP.Document.ToString()))` (BiliBiliComics:99) | 5-character replace, `& < > " '` → `&amp; &lt; &gt; &quot; &#39;` (FPC `EscapeHTML` ([source](https://gitlab.com/freepascal.org/fpc/source/-/blob/main/packages/fcl-xml/src/htmlelements.pp))). Hand-write it. |
| `DecodeBase64` | 23 / 17 | many | `base64`, made **lenient**: synapse's `Decode4to3Ex` tolerates junk and missing padding. |
| `EncodeURLElement` | 20 / 11 | query building | `percent-encoding` with a custom `AsciiSet`: `0x00–0x20 < > " % { } \| \ ^ [ ] \` 0x7F–0xFF ; / ? : @ = & # +`. `! * ' ( ) , $ ~` are **not** encoded. |
| `HMAC_SHA256` | 5 / 1 | PhiliaScans:36, `HMAC_SHA256(message, key)` | `hmac` + `sha2`. Note the **argument order is message first, key second**. |
| `HexToStr` | 4 / 3 | Happymh:29, MangaPlus:208, NewToki:74 | `hex`. FMD2 drops an odd trailing nibble and raises on non-hex input. |
| `EncodeBase64` | 4 / 3 | | `base64` STANDARD (padded) |
| `DecodeBase64URL` | 4 / 1 | MangaHub template:48 | `base64` URL_SAFE, padding optional |
| `SHA256` | 3 / 2 | Happymh:15 (raw 32 bytes) | `sha2` |
| `DecodeURL` / `EncodeURL` | 3 / 3, 2 / 2 | BlogTruyen, DigitalTeam | `%XX` triplet decode; `EncodeURL` uses only the URLSpecialChar set above |
| `AESCTR` | 2 / 1 | PhiliaScans:289 | `aes` + `ctr::Ctr128BE` (whole 16-byte counter, big-endian increment) |
| `AESDecryptGCM` | 1 / 1 | MangaHub template:52, `(cipher‖tag, key, iv[, aad])` | `aes-gcm` (see §4.3) |
| `X25519_PublicKey`, `X25519_SharedSecret` | 1 / 1 each | TheBlank:104 | `x25519-dalek` (see §4.3) |
| `SecretStream_InitPull`, `SecretStream_Pull` | 1 / 1 each | TheBlank:166,178 | libsodium `crypto_secretstream_xchacha20poly1305` pull (see §4.3) |
| `HMAC_SHA256Hex` | 1 / 1 | TheBlank:139 | `hmac` + `sha2` + lowercase hex |
| `HTMLDecode` | 1 / 1 | WPManga:112 | custom (see §4.3) |
| `MD5` | 1 / 1 | MangaTaro template:12 (raw 16 bytes) | `md-5` |
| `AESDecryptCBCSHA256Base64Pkcs7` | 1 / 1 | LunarManga:313, IV `'0'×32` hex | key = SHA-256(key string) → AES-256; IV = hex; `cbc` with **unchecked** PKCS#7 strip |

The other 34 functions have **no callers** but must exist for drop-in compatibility:

- `EncryptString`/`DecryptString`, `StrToHexStr`
- `MD5Hex`, `SHA1Hex`, `HMAC_SHA1Hex`, `SHA256Hex`
- `SHA512`/`SHA512Hex`, `HMAC_SHA512`/`HMAC_SHA512Hex`
- `AESEncryptCBC`/`AESDecryptCBC`, `AESEncryptCBCSHA256Base64Pkcs7`
- `AESDecryptCBCMD5Base64ZerosPadding`, `AESDecryptCBCHexBase64ZerosPadding`
- `AESEncryptECBPkcs7`/`AESDecryptECBPkcs7`, `AESCFB`, `AESOFB`
- `RC4`, `PBKDF2SHA256`, `EncodeBase64URL`, `AESEncryptGCM`
- `DecodeUU`/`EncodeUU`, `CRC16`, `CRC32`
- `MD4`, `HMAC_MD5`, `MD5LongHash`, `SHA1`, `HMAC_SHA1`, `SHA1LongHash`

The crates are `sha1`, `sha2`, `md-5`, `md4`, `hmac`, `pbkdf2`, `rc4`, `cfb-mode`, `ofb`, `ecb`, `cbc`, `aes-gcm` and `crc32fast`. They are all RustCrypto ([block-modes repo](https://github.com/RustCrypto/block-modes)) except `crc32fast`.

### 4.2 Cross-cutting quirks (replicate them; write tests from the Pascal)

- **AES key handling (DCPcrypt `TDCP_rijndael.InitKey`).** A raw key of any length up to 32 bytes is **zero-padded up to the next AES size**: ≤16 bytes → AES-128, ≤24 → AES-192, otherwise AES-256. A key longer than 32 bytes raises, so the call returns `''`. RustCrypto requires exact lengths, so pad before dispatching on the key size.
- **Raw IVs** in `AESEncryptCBC`, `AESDecryptCBC`, `AESCTR`, `AESCFB` and `AESOFB` are zero-padded or truncated to 16 bytes.
- **Empty input returns `''`.** Most functions return `''` if any argument is empty. `HMAC_*` returns `''` for an empty message or key, which is non-standard. **`SHA1Hex('')` returns `''`**, not the hash of the empty string.
- **Hex case.** Every `*Hex` function returns lowercase, but **`StrToHexStr` returns UPPERCASE** (`BinToHex`).
- **`AESEncryptCBC`/`AESDecryptCBC` use no padding.** A trailing partial block is handled DCPcrypt-style: `CV := E(CV)` and the tail is XORed, which is CFB-like and not standard CTS. RustCrypto's `cbc` has no equivalent, so hand-roll that last step.
- **`AESEncryptECBPkcs7`/`AESDecryptECBPkcs7` are buggy.** DCPcrypt's `EncryptECB` processes **one 16-byte block only**, so the bytes after the first block are uninitialised. There are no callers. Implement correct multi-block ECB and document the divergence.
- **`AESDecryptCBCMD5Base64ZerosPadding`** uses the **32-character lowercase hex string of MD5(key) as a raw 32-byte AES-256 key**. Despite its name it does **not** strip zero padding. `…HexBase64ZerosPadding` does strip trailing `\0` bytes.
- **PKCS#7 removal never validates.** It drops `ord(last byte)` bytes.
- **`HMAC_SHA1Hex` with a key over 64 bytes** pre-hashes the key to its **hex string** (40 bytes), not the 20-byte digest. That is non-standard, so hand-roll it. `HMAC_SHA256`/`HMAC_SHA512` follow the standard.
- **`AESCFB`** is full-block CFB-128 with a partial last segment, which maps to `cfb-mode`. **`AESOFB`** maps to `ofb`. **`PBKDF2SHA256`** is standard and maps to `pbkdf2::pbkdf2_hmac::<Sha256>`.

### 4.3 Notes on the rare and complex ones

- **`AESEncryptGCM`/`AESDecryptGCM`.** The GCM is hand-written, the tag is 16 bytes and appended, and decrypt returns `''` on tag mismatch.
  - FMD2 accepts **any IV length**: 12 bytes gives the standard J0, anything else is GHASHed.
  - `aes-gcm` fixes the nonce size at compile time (`AesGcm<Aes, NonceSize>`). Dispatch on 12 bytes and fall back to a generic instantiation, or hand-roll J0 with `ghash`.
  - The key length is variable, so dispatch over AES-128/192/256.
- **X25519.** FMD2 calls OpenSSL 3 (`libcrypto.so.3`) `EVP_PKEY` raw keys, which is outside Pascal. `x25519-dalek` `StaticSecret` / `PublicKey` / `diffie_hellman` is equivalent. One difference: OpenSSL fails, returning `''`, on an all-zero (non-contributory) shared secret, while dalek returns zeros. Check `SharedSecret::was_contributory()` to match.
- **SecretStream.** This is libsodium's `crypto_secretstream_xchacha20poly1305`, **pull only**, hand-written in Pascal on HChaCha20, ChaCha20 (IETF) and Poly1305.
  - Lua treats the state as an opaque string (FMD2's record is 64 bytes: k32, nonce12, pad20), so the layout need not match.
  - Options: `crypto_secretstream` 0.2.0 (RustCrypto nacl-compat, "pure Rust implementation of libsodium's crypto_secretstream" ([crates.io](https://crates.io/crates/crypto_secretstream))), or about 60 lines on `chacha20` + `poly1305`.
  - Pull returns `(state, msg, tag)`, or nothing on MAC failure. It rekeys on `TAG_REKEY` or counter wrap.
  - The Pascal pads the MAC input with `mlen & 15` zero bytes, which equals libsodium's `(0x10 - 64 + mlen) & 0xf`, so the two are consistent.
- **`HTMLDecode`** (uBaseUnit, not FPC):
  - It handles only `&amp; &lt; &gt; &nbsp; &quot;` and `&#NNN;`.
  - Numeric entities become a **single byte `Chr(N)`**, not UTF-8.
  - An unknown `&x` makes it `Exit` early, returning a garbage-tailed buffer, which is a bug.
  - Port the documented entities and stop decoding at an unknown entity. There is one caller.

### 4.4 `EncryptString` / `DecryptString`: exact scheme (needed for the FMD2 importer)

Source: `uBaseUnit.pas:1556-1590`, plus DCPcrypt (`dcpcrypt2.pas` `InitStr`/`EncryptString`, `dcpblockciphers.pas` `TDCP_blockcipher128.Init`/`EncryptCFB8bit`) ([NDXDeveloper/dcpcrypt-lazarus](https://github.com/NDXDeveloper/dcpcrypt-lazarus), the fork FMD2's README links).

```
passphrase = "B74945FB50E84FD58BF9FEAB8E4BEA6B"        (ASCII, hard-coded EncryptKey)
key        = SHA-512(passphrase)[0..32]                 (InitStr: digest 512 bits > MaxKeySize 256 → first 256 bits)
           = 973c529d79a03943b5045bd0ac5a715c05dee72cff2b9fe721092919a6578196
IV         = AES-256-ECB_key(0x00 × 16)                 (Init with InitVector=nil; DCP1COMPAT is off, so the fill is 0x00, not 0xFF)
           = 162094c6ec8e514a0e4afbf49fc6a7f1
cipher     = AES-256-CFB8(key, IV)                      (8-bit CFB with a 128-bit shift register; no padding, len(ct) = len(pt))
encoded    = standard Base64 with '=' padding           (DCPbase64)
EncryptString('') = ''                                 (and DecryptString('') = '')
```

- Test vectors were produced with `openssl enc -aes-256-cfb8 -K <key> -iv <IV> | base64` from the derivation above (script in the session scratchpad):
  - `hunter2` → `pcjhSQpguA==`
  - `user@example.com` → `uL4YtoWZ1Pjm4LggcWfMGw==`
- **Confirm them against a real FMD2-written value before relying on them.**
- In Rust: `sha2::Sha512` for the key, `aes::Aes256` for the IV, `cfb8::Decryptor<Aes256>` for the data, and `base64::STANDARD`.

Where FMD2 stores encrypted values:

- `userdata/modules.json` (`MODULES_FILE`): per-module `"Account": {"Username","Password","Cookies"}`, each one `EncryptString`'d (`WebsiteModules.pas:609-611, 671-673`).
- `settings.ini` `[connections] User/Pass`: proxy credentials (`frmMain.pas:5878, 6074`).
- The GitHub token used by `GithubRepo.pas:148`.
- Modules can also call `EncryptString`/`DecryptString` from Lua, although none do at baseline.

---

## 5. `fmd.gzip` and HTTP content-encoding

- **`fmd.gzip.Inflate(data)`** (`LuaGZip.pas` → `GZIPUtils.unzipStream`) **auto-detects the format**:
  - gzip header `1f 8b 08`: the header fields are skipped and the trailer CRC/ISIZE is cut but **not verified**.
  - zlib (`0x78`): the Adler-32 is cut.
  - anything else: raw deflate.
  - On failure it logs and returns nothing.
- **Usage:** 1 caller. Happymh:63 inflates its decrypted payload after a custom SHA-256-keystream XOR.
- **HTTP** (`httpsendthread.pas:600-700, 945`):
  - When `FCompress` is set, which is the default, it always sends `Accept-Encoding: gzip, deflate, br, zstd`.
  - It decodes the **whole buffered body** once, by substring sniffing of `Content-Encoding`: `zstd` → libzstd; else `br` → libbrotli; else `gzip`/`deflate` → `unzipStream` with auto-detection, so a raw "deflate" body works.
  - Decode errors are swallowed.
  - No module touches `Accept-Encoding` or compression.
- **Rust:**
  - `flate2` (the `miniz_oxide` or `zlib-rs` backend) with the same 3-way sniff.
  - `brotli-decompressor` (pure Rust).
  - `zstd` (vendored libzstd C) or `ruzstd` (pure-Rust decoder).
  - reqwest 0.13 has `gzip`/`brotli`/`zstd`/`deflate` features that auto-decode and add `Accept-Encoding` only when the request lacks one ([docs](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html)).
  - For fidelity, especially raw-deflate tolerance and FMD2's fixed `Accept-Encoding` string, **disable reqwest's auto-decoding and decode in the host after buffering**, exactly as FMD2 does. The HTTP ticket can revisit this.

---

## 6. `fmd.imagepuzzle`

- **API** (`LuaImagePuzzle.pas`, `ImagePuzzle.pas`):
  - `New/Create(hor, ver)`.
  - Array properties `Matrix[i]` and `Flips[i]`, **0-based**, passed through raw. `Matrix` defaults to the identity and `Flips` to 0.
  - `HorBlock`, `VerBlock` and a writable `Multiply` (default 1).
  - `DeScramble(inStream, outStream)`, where in and out may be the same `HTTP.Document`.
- **Algorithm:**
  - Decode WebP (libwebp), or else whatever LCL `TPicture` reads.
  - Allocate a white 32-bit canvas.
  - `blockW = w div hor` and `blockH = h div ver`. With `Multiply > 1`, these become `(w div (hor*m))*m` and the same for height.
  - Destination tile `Matrix[i]` receives source tile `i`. Flip bit 1 mirrors horizontally and bit 2 mirrors vertically.
  - Remainder pixels outside the grid stay **white**.
  - Output is **PNG if the input was WebP or PNG, otherwise JPEG** (LCL re-encode, lossy).
- **Users (7):** Comix, MangaGo, PhiliaScans, NexusScanlation (3 sites), PlusComico, WolfManga, TonarinoYoungJump (`Multiply = 8`).
- **Rust:** the `image` crate (webp via the pure-Rust `image-webp`, plus png and jpeg). Pixel output can be exact. The encoded bytes will differ: the JPEG quality should match LCL's default, which is not verified here and is a decision for the image-pipeline ticket.

## 7. `fmd.mangafoxwatermark` and `fmd.fileutil`

- **`mangafoxwatermark`:**
  - API: `LoadTemplate(dir)` returns a count, and `RemoveWatermark(filename[, saveAsPNG])` returns a bool. It runs on the saved file through `OnAfterImageSaved`.
  - Algorithm:
    - Templates are bottom-banner PNGs in `lua/extras/mangafoxtemplate/` (10 files).
    - For each template, the bottom-centred region is grey-scaled and Otsu-thresholded to 1-bit.
    - The region needs a white border of at least 4 rows.
    - PSNR is computed against the template; the best match must reach `MinPSNR = 9.0`.
    - On a match, the image is cropped by the template height and rewritten as JPEG or PNG.
  - About 500 lines of Pascal with pure pixel logic. Port it with `image`.
  - **Only user:** FanFox. It passes `fmd.LuaDirectory .. 'extras\\mangafoxtemplate'`, a **Windows backslash path** (see §8).
- **`fileutil`:** `ExtractFileName`, `ExtractFileNameOnly` and `SerializeAndMaintainNames(TStrings)`. Used by EHentai and Kurumizaka.
  - The first two are trivial.
  - `SerializeAndMaintainNames` (uBaseUnit:1669) checks whether the names are already sorted. If not, it zero-pads them to at least 3 digits and, if still unsorted, prefixes serial numbers. Port it line by line, because its exact output decides on-disk filenames.

## 8. `fmd.subprocess`, and a cross-ticket surprise

- **API** (`LuaSubprocess.pas`): `RunCommand(exe, args...)` and `RunCommandHide(...)`, which is the same with the window hidden.
  - They return `(ok, stdout, stderr, exitstatus)`, where `ok = (exitstatus == 0)`.
  - No shell is involved: the executable is resolved through `PATH`.
  - `New/Create` returns a bare TProcess with no methods exposed.
- **Callers (2), both Windows-specific as written:**
  1. **`websitebypass/cloudflare.lua:183`** runs `python lua\websitebypass\cloudflare.py <rooturl> --flaresolverr-ip … --flaresolverr-port … [--testing] [--debug]`.
     - The Python script needs `requests` and optionally `rookiepy`, and talks to FlareSolverr.
     - On Linux, `python` may not exist (only `python3`), and the backslash path does not resolve.
  2. **`utils/nodejs.lua:41`** runs `RunCommandHide("cmd.exe", "/c", ...)` for `node -v`, `mkdir`, `cd lua/utils/npm && npm install puppeteer`, and `node lua/utils/npm/tmp_scrpt.js`.
     - It **cannot work on Linux unmodified**, because there is no `cmd.exe`.
     - Users: **Comix and RaijinScans (always)** and the **Madara template when the `fullpageload` option is set**.
     - It also writes relative paths (`lua/utils/npm/...`), so it **depends on the process CWD** being the FMD2 root.
- **Rust:** `std::process::Command` with captured output.
- FMD2r must still decide whether to:
  - **(a)** accept these failures;
  - **(b)** add a host-level compatibility shim: translate `cmd.exe /c …` to `sh -c …`, normalise `\` in paths that modules hand to host functions (`RunCommand` args, `LoadTemplate`), and map `python` to `python3`;
  - **(c)** ship node, puppeteer and python in the Docker image.

  This touches the anti-bot, Docker and Lua-host tickets.

---

## Surprises that may change other tickets

1. **`fmd.pcre2` has zero callers.** It is cheap to provide, but it should not drive any design decision.
2. **Madara's `ExecJS` path is used by 125 modules** and needs a CommonJS `require` that resolves to `lua/utils/*.js`. The JS engine must emulate Duktape's module loader, not just evaluate code.
3. **MangaPlus locates its `.proto` through `debug.getinfo(1,'S').source`.** Module chunks must be loaded with chunkname `@<absolute path>`.
4. **Windows-isms in "unmodified" modules:**
   - `cmd.exe /c` in `utils/nodejs.lua` (Comix, RaijinScans, Madara `fullpageload`).
   - `python lua\websitebypass\cloudflare.py`.
   - `'extras\\mangafoxtemplate'` (FanFox).
   - CWD-relative `lua/...` paths.

   Drop-in compatibility on Linux therefore needs **host-side path and command shims** or explicit acceptance of these failures.
5. **X25519 goes through OpenSSL 3** (`libcrypto.so.3`), not Pascal. The rest of `fmd.crypto` is DCPcrypt plus hand-rolled GCM and secretstream. Everything maps to RustCrypto, but there are **many non-standard quirks**: zero-padded AES keys, single-block ECB, hex-keyed HMAC-SHA1, `SHA1Hex('')=''`, uppercase `StrToHexStr`, and an MD5-hex AES key. A pure-crate port would silently differ, so Pascal-derived test vectors are needed.
6. **`EncryptString` is fully reproducible without Pascal.** The key, IV and test vectors are in §4.4, so the importer can decrypt `modules.json` accounts and proxy credentials.
7. **`ExecJS` has no timeout or memory limit upstream** and runs untrusted site JS. FMD2r should add both, which rquickjs supports.
8. **The shipped Duktape is 2.7.0, not the 2.3.0 the Pascal headers claim.** This only matters if we ever pick Duktape as an oracle.
