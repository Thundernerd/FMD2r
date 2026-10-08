# Globals and `fmd.*` libraries

References point at upstream FMD2 `ad3a5b63`. Usage is `occurrences / files` across `lua/`. Global counts come from `luac -l` (global reads); library member counts come from a member-name grep ([usage-counts.md](usage-counts.md#method)).

## 1. Base globals (every state)

These are registered by `LuaBaseRegisterAll` (`LuaBase.pas:73-92`), `luaBaseUnitRegister` (`LuaBaseUnit.pas`) and `luaSynaUtilRegister` (`LuaSynaUtil.pas`). Unless the table says otherwise, string arguments go through `luaToString`, which truncates at NUL and turns non-strings into `''`.

| Global | Signature | Semantics | Uses |
|---|---|---|---|
| `print(...)` | any → none | Each argument is written as a **separate** log line through `SendLog`. Booleans become `true`/`false`, everything else goes through `luaToString` (`nil` and tables become `''`). This **replaces** Lua's `print`. | 51 / 14 |
| `sleep(ms)` | int → none | Blocks the thread for `ms` milliseconds. | 15 / 12 |
| `CreateTXQuery([html])` | → TXQuery | See [objects.md §11](objects.md#11-txquery-and-ixqvalue-method-surface-only). | 1159 / 347 |
| `Trim(s[, chars])` | → string | With 1 argument, FPC `Trim`: strips characters `<= ' '` from both ends. With 2 or more, strips any character of `chars` from both ends. | 21 / 16 |
| `MaybeFillHost(host, url)` | → string | Runs `SplitURL(url)`. If `url` has no host part and a non-empty path, returns `host` without its trailing `/` plus the path. Otherwise returns `url` unchanged (`uBaseUnit.pas:943-950`). Host detection follows FMD2's own `SplitURL` (`httpsendthread.pas:191-277`), which **FMD2r should port verbatim**. It treats `//host/p`, `scheme://…`, `host:port/…`, and a first segment containing a dot followed by `/` as having a host. A bare `a.b.c` with two dots and no `/` also counts as a host. A host without a scheme gets `https://`. | 647 / 277 |
| `MangaInfoStatusIfPos(s[, ongoing[, completed[, hiatus[, cancelled]]]])` | → string | See [below](#mangainfostatusifpos). | 220 / 218 |
| `GetBetween(open, close, s)` | → string | Synapse `GetBetween` (`synautil.pas:1671-1723`). **The argument order is (open, close, text).** Returns the text between the first `open` and its matching `close`, tracking nesting when `open` occurs again before `close`. Returns `s` unchanged when either delimiter is missing. Returns `''` when `s == open..close`. | 75 / 44 |
| `SeparateLeft(s, delim)` | → string | The part before the first `delim`, or `s` if `delim` is absent. | 5 / 4 |
| `SeparateRight(s, delim)` | → string | The part after the first `delim`, or `s` if `delim` is absent. | 10 / 5 |
| `NewWebsiteModule()` | → module | **Only exists in Init states**, registered just before `Init()` is called ([runtime.md §2.1](runtime.md#21-init-states-startup-one-per-module-file)). | 623 / 623 |

### `MangaInfoStatusIfPos`

Defaults: `ongoing='ongoing'`, `completed='complete'`, `hiatus='hiatus'`, `cancelled='cancel'`.

1. Lower-cases `s` and each pattern.
2. A pattern containing `|` is a list of alternatives.
3. Tests the patterns as **substrings**, in the order ongoing, completed, hiatus, cancelled.
4. Returns `'1'` for ongoing, `'0'` for completed, `'2'` for hiatus, `'3'` for cancelled, and otherwise **`RS_InfoStatus_Unknown` = `'Unknown'`**, which is a *translatable* resource string (`frmMain.pas:1010`).
5. If `s` is `''` it returns `''`. With 0 arguments it returns nothing (`uBaseUnit.pas:2793-2851`).

### Per-hook globals

Per-hook globals (`MODULE`, `HTTP`, `URL`, `NAMES`, `LINKS`, `MANGAINFO`, `TASK`, `UPDATELIST`, `PAGENUMBER`, `WORKPTR`, `WORKID`, `PATH`, `FILENAME`, `MANGACHECK`) and the status and account constants are listed in [hooks.md](hooks.md). Their read counts:

| Global | Uses |
|---|---|
| `MANGAINFO` | 3337 / 334 |
| `HTTP` | 3152 / 368 |
| `MODULE` | 1631 / 333 |
| `URL` | 927 / 330 |
| `TASK` | 583 / 308 |
| `NAMES` | 301 / 291 |
| `LINKS` | 301 / 291 |
| `UPDATELIST` | 92 / 72 |
| `WORKID` | 52 / 24 |
| `PAGENUMBER` | 8 reads / 161 writes |
| `MANGACHECK` | 5 / 1 |
| `FILENAME` | 1 / 1 |
| `WORKPTR` | 0 |
| `PATH` | 0 |

## 2. `fmd.*` libraries

Libraries load through `require 'fmd.<name>'` ([runtime.md §4.1](runtime.md#41-searcher-installation-luapackagepas453-506)). Each returns a plain table of C functions. Lookup is case-insensitive, but the convention is lower case. Require counts:

| Library | Uses |
|---|---|
| `fmd.crypto` | 84 / 64 |
| `fmd.env` | 70 / 70 |
| `fmd.duktape` | 9 / 8 |
| `fmd.imagepuzzle` | 8 / 7 |
| `fmd.fileutil` | 3 / 2 |
| `fmd.subprocess` | 2 / 2 |
| `fmd.mangafoxwatermark` | 2 / 1 |
| `fmd.gzip` | 1 / 1 |
| `fmd.logger` | 1 / 1 |
| `fmd.pcre2` | **0** |
| `fmd.strings` | **0** |

### `fmd.env` (`LuaFMD.pas`)

`fmd.env` returns a table of strings, built fresh on each first `require` in a state.

| Field | Value in FMD2 | Uses |
|---|---|---|
| `Directory` | FMD2 install directory, with a trailing separator | 0 |
| `ExeName` | Executable name without extension | 0 |
| `Version` | Program version string | 0 |
| `Revision` | `REVISION_NUMBER`: the upstream git first-parent commit count as a string (6930 at the baseline; `''` for builds without git, `git2revision.bat`) | 1 (`templates/MangaHub.lua:122` requires `tonumber(Revision) >= 6920`, otherwise GetPageNumber fails) |
| `LuaDirectory` | `<Directory>lua/` | 1 (`modules/FanFox.lua:36`, concatenated with the Windows-style `'extras\\mangafoxtemplate'`) |
| `SelectedLanguage` | UI language code, such as `en` (`SimpleTranslator.LastSelected`; `''` until a language is selected) | 68 / 68 (Init option-caption localisation) |

**FMD2r:** `Revision` must be a number at least as large as the upstream revision of the module set it runs. Reporting FMD2r's own version here would break `MangaHub` modules.

### `fmd.crypto` (`LuaCrypto.pas`, `BaseCrypto.pas`, Synapse `synacode.pas`)

Every argument is read with `luaL_checklstring`. That is **binary-safe**, and a non-string argument **raises a Lua error** (numbers are accepted). Results are pushed binary-safe. "hex" means a lower-case hex string. Argument order is `(data, key[, iv])`, so **HMAC takes the message first and the key second**.

| Function | Signature → result | Uses |
|---|---|---|
| `HTMLEncode(s)` | escape HTML | 47 / 29 |
| `DecodeBase64(s)` | → bytes | 23 / 17 |
| `EncodeURLElement(s)` | percent-encode a URL component (Synapse) | 20 / 11 |
| `HMAC_SHA256(msg, key)` | → raw | 5 / 1 |
| `HexToStr(hex)` | → bytes | 4 / 3 |
| `DecodeBase64URL(s)` | → bytes | 4 / 1 |
| `EncodeBase64(s)` | → string | 4 / 3 |
| `SHA256(s)` | → raw 32 bytes | 3 / 2 |
| `DecodeURL(s)` | percent-decode | 3 / 3 |
| `AESCTR(data, key, iv)` | | 2 / 1 |
| `EncodeURL(s)` | Synapse URL encoding | 2 / 2 |
| `HTMLDecode(s)` | | 1 |
| `HMAC_SHA256Hex(msg, key)` | | 1 |
| `AESDecryptCBCSHA256Base64Pkcs7(b64, key, iv)` | | 1 |
| `AESDecryptGCM(data, key, iv[, aad])` | | 1 |
| `X25519_PublicKey(priv)` | via OpenSSL `libcrypto` (`EVP_PKEY_X25519`) | 1 |
| `X25519_SharedSecret(priv, pub)` | | 1 |
| `SecretStream_InitPull(header, key)` | → state, or `nil` | 1 |
| `SecretStream_Pull(state, chunk)` | → `(state, msg, tag)`, or nothing on failure | 1 |
| `MD5(s)` | → raw 16 bytes (Synapse) | 1 |
| Unused | `EncryptString`, `DecryptString` (FMD2's own reversible obfuscation, also used for `modules.json` account fields); `StrToHexStr`, `MD5Hex`, `SHA1Hex`, `HMAC_SHA1Hex`, `SHA256Hex`, `SHA512`, `SHA512Hex`, `HMAC_SHA512`, `HMAC_SHA512Hex`; `AESEncryptCBC`, `AESDecryptCBC` (no padding handling), `AESEncryptCBCSHA256Base64Pkcs7`, `AESDecryptCBCMD5Base64ZerosPadding`, `AESDecryptCBCHexBase64ZerosPadding`, `AESEncryptECBPkcs7`, `AESDecryptECBPkcs7`, `AESCFB`, `AESOFB`, `RC4`; `PBKDF2SHA256(pw, salt, iter, dkLen)`, `EncodeBase64URL`, `AESEncryptGCM`; `DecodeUU`, `EncodeUU`, `CRC16`, `CRC32` (→ integer), `MD4`, `HMAC_MD5`, `MD5LongHash(s, len)`, `SHA1`, `HMAC_SHA1`, `SHA1LongHash(s, len)` | 0 |

The exact key-derivation conventions of the composite AES helpers, for example the key and IV derivation in `AESDecryptCBCSHA256Base64Pkcs7`, are defined in `BaseCrypto.pas`. FMD2r must port that file function by function.

### `fmd.duktape` (`LuaDuktape.pas`, `Duktape.pas`)

| Function | Semantics | Uses |
|---|---|---|
| `ExecJS(code)` | See below. | 13 / 8 (plus `require 'fmd.duktape'.ExecJS` in Madara) |

`ExecJS(code)`:

* Creates a **fresh Duktape heap for each call** and destroys it afterwards.
* Initialises Duktape's CommonJS module loader (`duk_module_duktape_init`) with a native `Duktape.modSearch`. The module search resolves `require(id)` to the file `lua/<id>` or `lua/<id>.js`. That path is **relative to the process CWD** (`DukLibDir = 'lua/'`). Files are cached process-wide and evaluated as CommonJS module bodies.
* Defines a global `print` that writes to the FMD log.
* Runs `code` as a **program** with `duk_peval`, so top-level `var` and function declarations become globals.
* Returns the completion value of the last expression statement, converted with `ToString`. `undefined` becomes `''`.
* On a JS error it logs `Duktape.ExecJS() Duktape error: …` and **returns no values**.

Modules use `require('utils/crypto-js.min.js')` inside JS (`modules/MangaGo.lua:206`, `templates/Madara.lua:178`).

### `fmd.imagepuzzle` (`LuaImagePuzzle.pas`)

`New(h, v)` and `Create(h, v)` both return a puzzle object ([objects.md §12](objects.md#12-imagepuzzle-from-require-fmdimagepuzzle)). `Create` has 10 uses in 8 files, shared with other `Create` members. `New` has 0.

### `fmd.fileutil` (`LuaFileUtil.pas`)

| Function | Semantics | Uses |
|---|---|---|
| `ExtractFileName(path)` | FPC: the part after the last directory separator. On Windows both `\` and `/` count. | 0 |
| `ExtractFileNameOnly(path)` | The same, without the extension. | 2 / 2 |
| `SerializeAndMaintainNames(list)` | Takes a TStrings userdata (unchecked; a non-userdata argument is a no-op). If the names don't already sort in list order, it zero-pads to `max(3, digits(count))` and/or prefixes serial numbers so that lexical order equals list order (`uBaseUnit.pas:1669-…`). | 1 (`EHentai.lua`, on `TASK.FileNames`) |

### `fmd.gzip` (`LuaGZip.pas`)

| Function | Semantics | Uses |
|---|---|---|
| `Inflate(data)` | Binary-safe. Input may be gzip, zlib or raw deflate (`unzipStream` sniffs). Returns the bytes, or **no value** on failure (logged). | 1 / 1 |

### `fmd.logger` (`LuaLogger.pas`)

| Function | Uses |
|---|---|
| `Send(msg)` | 0 |
| `SendWarning(msg)` | 2 / 2 |
| `SendError(msg)` | 10 / 1 |

Each one writes to the FMD log at the matching level. Its only `require` is in `websitebypass.lua`, which stores the table in the **global** `LOGGER` (12 reads, in the bypass scripts).

### `fmd.mangafoxwatermark` (`LuaMangaFox.pas`)

| Function | Semantics | Uses |
|---|---|---|
| `LoadTemplate(dir)` | → int, the number of templates loaded | 1 |
| `RemoveWatermark(file[, saveAsPNG=false])` | → bool | 1 |

Only `modules/FanFox.lua` uses it. This is image-pipeline work.

### `fmd.subprocess` (`LuaSubprocess.pas`)

| Function | Semantics | Uses |
|---|---|---|
| `RunCommand(exe, arg1, …)` | Runs `exe` with the arguments (no shell) and waits. Returns `ok, stdout, stderr, exitStatus`. `ok` is false when the run failed or `exitStatus ≠ 0`. | 0 |
| `RunCommandHide(exe, …)` | Same, with a hidden console window. | 2 / 2 (`utils/nodejs.lua` runs `cmd.exe /c …`; `websitebypass/cloudflare.lua` runs Python/FlareSolverr) |
| `New()` / `Create()` | A `TProcess` with no members ([objects.md §13](objects.md#13-tprocess-from-require-fmdsubprocess)). | 0 |

### `fmd.pcre2` (`LuaPCRE2.pas`, **0 callers**)

Patterns compile with `PCRE2_UTF`. An optional `init` argument is 1-based. A compile error is logged and the function returns `false` or nothing.

| Function | Semantics |
|---|---|
| `exec(s, pat[, init])` | → bool |
| `find(s, pat[, init])` | → `start, end` (1-based, inclusive), or nothing |
| `match(s, pat[, init])` | → the whole match if the pattern has no groups, else all groups |
| `gmatch(s, pat)` | → iterator over matches (Lua-style) |
| `gsub(s, pat, repl[, init])` | `pcre2_substitute` with `SUBSTITUTE_GLOBAL` and PCRE2 replacement syntax (`$1`) |

FMD2r may implement this lazily or leave it unimplemented, and fail loudly if it is ever called, per map #1.

### `fmd.strings` (`LuaStrings.pas`, **0 callers**)

`New()` / `Create()` → a new auto-freed `TStringList` userdata ([objects.md §5](objects.md#5-tstrings-tstringlist)).

## 3. Non-`fmd` native requires

* `require 'pb'`: lua-protobuf, a C module loaded through `package.cpath` (`dist/*/pb.dll`). It is used by `utils/protoc.lua` and `modules/MangaPlus.lua`.
* `require 'utf8'`: the stdlib table, already in `package.loaded`. Used once.
* Several `require(...)` strings inside JavaScript text (`puppeteer`, `vm`, `http`, `fs`, `utils/crypto-js.min.js`) belong to Duktape or Node.js. They are not Lua requires.
