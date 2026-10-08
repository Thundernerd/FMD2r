# Host API specification (upstream FMD2 `ad3a5b63`)

Research for [#3 "Specify the Host API"](https://github.com/Thundernerd/FMD2r/issues/3), part of map #1.

The **Host API** is everything an upstream Lua **Module** can see or call that the FMD2 host provides: globals, userdata objects, hooks, `fmd.*` libraries, the `require` machinery and the Lua state lifecycle. FMD2r must reproduce it closely enough that the files under upstream `lua/` run **unmodified**.

All file and line references point at the upstream checkout `~/Repositories/Forks/FMD2` at commit `ad3a5b63` ("Tapas: Fixed all (#3281)"). Pascal sources are under `baseunits/` (bindings in `baseunits/lua/`). Lua sources are under `lua/`. Usage counts come from 676 `.lua` files under `lua/` (624 in `modules/`, 41 in `templates/`, 7 in `utils/`, 4 in `websitebypass/`, 1 in `extras/`). The method is described in [usage-counts.md](usage-counts.md#method).

## Files

| File | Contents |
|---|---|
| [runtime.md](runtime.md) | Lua version and stdlib. Lua state lifecycle per loader thread, per worker thread and per module. `Init`/`NewWebsiteModule`. `package.searchers` and the bytecode caches. The userdata calling convention (`__index`/`__newindex`/`__get`/`__set`/`__defaultget`, upvalue-bound self, ignored setters). Type coercion. Error propagation. |
| [hooks.md](hooks.md) | Every `MODULE.On*` hook: when the host calls it, which globals it sets beforehand, how the return value is read, and what the host does with the result. Includes `OnCheckSite`/`MANGACHECK` and the anti-bot hooks `____CheckAntiBot`/`____WebsiteBypass`. |
| [objects.md](objects.md) | Every userdata class: `MODULE` (TLuaWebsiteModule), `Account`, `Storage`, `HTTP` (THTTPSendThread, with full request semantics), `TStrings`, `TMemoryStream`, `MANGAINFO`, `TASK`, `UPDATELIST`, `MANGACHECK`, `TCriticalSection`, `TXQuery`/`IXQValue` (method surface only), `ImagePuzzle`, `TProcess`. Includes properties, methods and usage counts. |
| [globals-and-libraries.md](globals-and-libraries.md) | Base globals (`print`, `sleep`, `CreateTXQuery`, `Trim`, `MaybeFillHost`, …), the status and account constants, and every `fmd.*` library (`env`, `crypto`, `duktape`, `pcre2`, `strings`, `logger`, `fileutil`, `gzip`, `imagepuzzle`, `mangafoxwatermark`, `subprocess`), with usage counts. |
| [usage-counts.md](usage-counts.md) | Method, global-read inventory, `require` inventory, load-bearing vs unused summary, and module bugs that FMD2r must not "fix". |

## Executive summary

* **Engine.** Release builds use **Lua 5.4** (`-dlua54`, `dist/*/lua54.dll`). The LuaJIT build flag is disabled (`mangadownloader/md.lpi:292-293`). Modules use 5.3+ syntax such as the `~` xor operator (`modules/MangaPlus.lua:215`). FMD2r needs Lua 5.4 with the full stdlib (`luaL_openlibs`).
* **Calling convention.** Host objects are full userdata. Each one gets its **own fresh metatable**, and every method and property accessor in it is a C closure whose upvalue 1 is the object. Methods are therefore **dot-called** (`HTTP.GET(url)`). Colon calls would pass the userdata as argument 1 and shift every argument, and **no file in `lua/` uses one**. Property reads and writes go through `__index`/`__newindex`. Writing an unknown key is **silently ignored**. Reading an unknown key returns the object's **metatable** (a truthy table), not `nil`. See [runtime.md §5](runtime.md#5-the-userdata-calling-convention-luaclasspas).
* **Lua states.** Each worker thread keeps **one Lua state** for the module it last served. Switching to a different module closes the state and builds a new one. Globals therefore persist between hook calls of the same module on the same thread. `Init` runs once per file in a throwaway state at startup. See [runtime.md §2](runtime.md#2-lua-state-lifecycle).
* **Hooks.** `MODULE.OnX = 'FunctionName'` stores a **global function name**. The host sets per-call globals (`HTTP`, `URL`, `TASK`, …) and then calls the function. It reads the **top stack slot**, which is the *last* return value. Integer hooks treat a missing return as `0` (`no_error`). See [hooks.md](hooks.md).
* **What modules lean on.** `HTTP.GET` (1183 uses), `HTTP.Document` (1329), the `CreateTXQuery` XPath wrapper (1159 uses, plus 2242 `XPathString`), the `TStrings` objects `NAMES`/`LINKS`/`MANGAINFO.Chapter*`/`TASK.PageLinks` (`Add` 900, `Values` 306, `Reverse` 413), `MaybeFillHost` (647), `fmd.env` (70 files) and `fmd.crypto` (64 files). Large parts of the surface go **unused**: `fmd.strings`, `fmd.pcre2`, all critical-section methods, `HEAD`, `SetProxy`, most crypto functions and the `OnSaveImage` hook. See [usage-counts.md](usage-counts.md).

## Surprising findings worth carrying into other tickets

1. **`HTTP.GET`'s return value means "the body is non-empty", not "the request succeeded".** A 404 page with a body returns `true`. A 200 with an empty body returns `false`. 5xx responses are retried `RetryCount` times. For module HTTP objects, however, the anti-bot wrapper sets `AllowServerErrorResponse := True`, which turns those retries off. (`httpsendthread.pas:592-718`, `LuaWebsiteBypass.pas:156`.) This matters for the runtime (#11) and anti-bot (#6) tickets.
2. **Requests auto-reset.** If `HTTP.Headers` still holds the previous *response* (its first line starts with `HTTP/`), the next `GET`/`POST`/`XHR` silently calls `Reset()`. That wipes request headers, cookies and `Document`, so a POST body written after an earlier request without an explicit `HTTP.Reset()` is lost. Modules depend on this ordering (130 `HTTP.Reset()` calls).
3. **`HTTP.Request(method, url)` bypasses the anti-bot wrapper. `GET`/`POST`/`XHR`/`HEAD` go through it.** The bypass scripts rely on this to avoid recursion.
4. **Most host functions truncate strings at the first NUL byte** because `luaToString` converts via `PAnsiChar`. The binary-safe exceptions are `fmd.crypto`, `fmd.gzip`, `Document.WriteString`, and `fmd.pcre2` input/output. Binary data must be routed accordingly.
5. **`fmd.env.Revision` gates behaviour.** `templates/MangaHub.lua:122` refuses to work if `tonumber(Revision) < 6920`. Upstream's revision is the first-parent commit count (6930 at baseline, `git2revision.bat`). FMD2r must report an upstream-compatible revision number, not its own version.
6. **`require 'pb'` loads a native C module** (lua-protobuf `pb.dll`, shipped in `dist/`) through Lua's standard C searcher (`modules/MangaPlus.lua:54`, `utils/protoc.lua:993`). This is a native dependency for the native-dependencies ticket (#9) and the JS/native usage ticket (#5).
7. **The XPath wrapper swallows every evaluation error and returns an empty sequence** (`XQueryEngineHTML.pas` `Eval`: `try … except end`). 145 files rely on `tonumber(x.XPathString(…)) or N`. **`json("http://…")` cannot fetch.** It would go through Internet Tools' own `defaultInternet` client, not FMD2's `HTTP` object. FMD2 links no internetaccess backend, so the call fails, and the failure is swallowed. No module uses it.
8. **Host-side exceptions abort the whole hook.** For example, `TStrings` index out of range, `IXQValue.GetAttribute` on a non-node, or a `nil` passed where a stream is expected. Each of these is a Pascal exception that unwinds through Lua. Lua `pcall` cannot catch it. The hook returns its failure default (`INFORMATION_NOT_FOUND`, `false` or `''`).
9. **Upstream quirks that a drop-in port has to keep or knowingly drop:**
   * The update-list driver calls `OnAfterUpdateList` *before* `OnBeforeUpdateList` at the start of a run (`uUpdateThread.pas:671-674`).
   * In `GetNameAndLink`, `URL` is the **0-based page index as a string**.
   * `OnCheckSite` is wired to `DoCheckSite`, but the only real caller is the debug **Check Modules** form, which calls the function directly with a `MANGACHECK` object.
10. **Modules contain dead writes and undefined reads that must stay harmless.** Examples: `HTTP.FollowRedirection = …` (not bound, so silently ignored, `websitebypass/cloudflare.lua:134`); `MANGAINFO.Artist` (typo); reads of undefined globals such as `net_error`, `WorkId` and `Module`. A port must not turn unknown-key writes or reads into errors.
