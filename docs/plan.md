# FMD2r — Rust port of FMD2 with a web UI

## Context

FMD2 (`~/Repositories/Forks/FMD2`, Lazarus/Free Pascal, Windows-centric desktop app) is valuable mostly because of its
~665 community-maintained Lua website modules (`lua/modules`, `lua/templates`, `lua/utils`, `lua/websitebypass`),
which are updated far more often than the program itself. FMD2r rewrites the program as a **self-hosted, single-user
Rust backend (SQLite) with a mobile-friendly Svelte web UI** (layout: prototype variant B "Library",
https://claude.ai/artifact/As3XTN7NjxM8ydAP9oEcqL), while running the upstream Lua modules **unchanged** and pulling
updates straight from the upstream repo (`dazedcat19/FMD2`, path `lua/`).

The governing rule: **the Lua Host API is the contract.** Every other part of FMD2 can be redesigned; the globals,
objects, callbacks and their quirks that modules see must be reproduced faithfully.

Decisions taken:
- XPath: wrap FMD2's own engine (internettools) as a C-ABI shared library first; replace it with a Rust engine later, gated by differential tests.
- Deployment: one binary that serves the API and the embedded SPA, plus a Docker image. Optional password/token auth. No user accounts.
- Lua: `mlua` with vendored Lua 5.4. C modules are allowed, because `pb` is needed.
- JavaScript (`fmd.duktape.ExecJS`): `rquickjs`, a C engine with a Rust API and an ES5 superset. Duktape via `cc` is the fallback if QuickJS behaves differently.
  - T12 kept QuickJS: the upstream snippets (packed `eval`, crypto-js `require`, JSON state) evaluate as under Duktape once scripts run as non-strict global code (rquickjs defaults to strict, which makes `eval("var x")` local). Known, unused-by-modules differences: no `Duktape` global object (`modLoaded`/`modSearch`) and no `module.filename`/`module.name`; non-BMP characters are UTF-16 surrogate pairs (`length` 2, returned as 4-byte UTF-8) where Duktape 2.3 keeps its own extended UTF-8 (unverified against a Duktape build); error messages and `Function.prototype.toString` text differ, and QuickJS adds ES2015+ built-ins that feature-detecting scripts may pick up. Each `ExecJS` is bounded by a time and memory limit and the worker's `TerminateToken`; Duktape has none.
- Storage: `rusqlite`, with a fresh normalized schema. FMD2 data comes in through a one-time importer; FMD2's file layout is not reused.

## Architecture (Cargo workspace)

```
crates/
  fmd-lua      Lua runtime + full Host API (the core; see below)
  fmd-xpath    trait XPathEngine + backends: `fpc` (FFI to libfmdxpath.so), later `native`
  xpath-fpc/   tiny FPC library project exporting internettools over a C ABI (built by build.rs / CI)
  fmd-http     blocking HTTP façade with Synapse semantics on top of reqwest (gzip/br/zstd, socks/http proxy, cookie jar per module, per-module connection queue)
  fmd-store    rusqlite schema + migrations + repositories (app.db, lists.db)
  fmd-core     domain: modules registry, download engine, favorites checker, list updater, module updater, scheduler, inbox/events
  fmd-pack     output: folder / zip / cbz (zip crate), pdf (lopdf or printpdf), epub (custom on zip); image conversion via `image` (+ libwebp-sys if needed)
  fmd-import   FMD2 userdata importer (downloads.db, favorites.db, downloadedchapters.db, modules.json, settings.json)
  fmd-server   axum: REST (OpenAPI via utoipa) + SSE event stream + cover proxy/cache + embedded SPA (rust-embed)
  fmd2r        binary: `serve`, plus dev CLI subcommands (`module init|info|pages|download`, `xpath eval`)
web/           SvelteKit (Svelte 5, adapter-static SPA), API client generated from OpenAPI
```

### Concurrency model
- Lua states are `!Send`, so modules run on a **pool of dedicated OS worker threads**, the same model as FMD2. Each worker owns one Lua state, cached per module and rebuilt when the worker switches to a different module. Globals therefore persist between callbacks of the same module on the same thread, as modules expect.
- The async side (axum, scheduler) sends jobs to the workers over channels. Inside a worker, HTTP is blocking. A `reqwest` client on a shared tokio runtime is driven with `Handle::block_on`.
- Limits copy FMD2: max parallel tasks, threads per task, and the module's `MaxTaskLimit`, `MaxThreadPerTaskLimit` and `MaxConnectionLimit`, overridable per module.

## fmd-lua: Host API to reproduce (reference: `baseunits/lua/*.pas`)

**Loading.** Port `LuaWebsiteModules.pas:473-656`:
- Scan `lua/modules/*.lua` in parallel. Each file runs in a fresh state: execute the chunk, then register the global `NewWebsiteModule`, then call `Init()`.
- One file may create several modules. Drop modules with no ID or Name, and lowercase RootURL.
- Copy the values set on `m` into a Rust `ModuleDef`: properties, `On*` callback names and options.
- Install a custom `package.searchers[1]`: `fmd.<lib>` resolves to a host lib, anything else to `lua/<name with . → />.lua`. Keep the standard searchers. `pb` is compiled with `cc` and registered in `package.preload`.

**Callbacks** run by name with no arguments, with these globals set first:
- Update list: `OnGetDirectoryPageNumber` (`PAGENUMBER`, read back afterwards), `OnGetNameAndLink` (`URL` = page index *as a string*), `OnBefore/AfterUpdateList`.
- Manga info: `OnGetInfo` (`MANGAINFO.URL` pre-filled).
- Downloading: `OnTaskStart`, `OnGetPageNumber`, `OnGetImageURL`, `OnBeforeDownloadImage`, `OnDownloadImage`, `OnSaveImage`, `OnAfterImageSaved` (with `WORKID`).
- Accounts: `OnLogin`, `OnAccountState`, `OnCheckSite`, plus the `no_error/net_problem/information_not_found` and `as*` constants.
- Run a full GC every 16 calls.

**Object binding semantics** (from `LuaClass.pas`). Build these once in a generic `LuaClass` helper on mlua userdata and metatables:
- Methods are closures bound to the object, so **dot calls** work (`HTTP.GET(u)`). A redundant leading self argument is stripped, so colon calls also work.
- Properties go through `__index`/`__newindex`. Unknown keys fall through to the default array property, or are **silently ignored**.
- Keys are case-sensitive. Every object has `.self()`.
- The default `obj[i]` on TStrings is **0-based**. `IXQValue.Get(i)` is 1-based, and `Get()` returns an iterator.
- Strings stay binary-safe. Crypto needs that; FMD2 truncates at NUL only in specific places.

**Globals and libraries:**

| Group | Members |
|---|---|
| Globals | `print`, `sleep`, `Trim`, `MaybeFillHost`, `MangaInfoStatusIfPos`, `GetBetween`, `SeparateLeft`, `SeparateRight`, `CreateTXQuery` |
| `HTTP` | GET / POST / HEAD / XHR / Request / Reset / ResetBasic, cookie methods, SetProxy. Properties: Headers, Cookies, Document (memory stream), ResultCode, LastURL, UserAgent, MimeType, RetryCount, EnabledCookies, Terminated |
| `MANGAINFO` | URL, Title, AltTitles, Link, CoverLink, Authors, Artists, Genres, Status, Summary, ChapterNames, ChapterLinks |
| `TASK` | PageLinks, ChapterLinks, ChapterNames, PageContainerLinks, FileNames, CurrentDownloadChapterPtr, PageNumber, CurrentMaxFileNameLength, Link |
| `UPDATELIST` | CurrentDirectoryPageNumber, UpdateStatusText |
| `MODULE` | properties; AddOption* / GetOption; Add/Get/Remove/ClearCookies; Storage (thread-safe key/value); Guardian (critical section); Account |
| TStrings | full surface, including `fmd.strings.New` |
| `fmd.*` libs | env, crypto, duktape, gzip, fileutil, imagepuzzle, mangafoxwatermark, logger, subprocess, pcre2 (unused by modules; low priority) |

**HTTP quirks** to reproduce exactly (`httpsendthread.pas:592-946`):
- Add `https://` when the URL has no scheme.
- Retry on errors and on status >500.
- Follow 301/302/303/307 as GET, at most 5 times, adding a Referer.
- `GET` returns **true when the body is non-empty**, even on a 404.
- `Headers` holds the request headers before a call and the response headers after it. The next request resets it when it starts with `HTTP/`.
- `POST` moves a `Content-Type` header into MimeType; `text/html` becomes form-urlencoded.
- Run the anti-bot hook after every request: `checkantibot.lua`, then `websitebypass.lua`. Serialize it per module. On success, store the cookies and user agent in the module's settings, then retry if `MODULE.Storage['reload']` is set.

**Crypto.** Implement with RustCrypto crates. Prioritize the functions modules actually call:
- EncodeURLElement, Base64 and Base64URL, HTMLEncode/HTMLDecode, HMAC_SHA256, SHA256, HexToStr, AESCTR, AESDecryptGCM.
- X25519 and SecretStream (libsodium-compatible; `crypto_secretstream` crate or libsodium-sys).
- The remaining functions after those.

**XPath** (`CreateTXQuery`, `IXQValue`): delegate to `fmd-xpath`. The FPC shim exports parse, eval and value-handle functions. Configure it exactly like `XQueryEngineHTML.pas:384-400`: HTML repair, trimText=false, no comments, and errors yield an empty value. `XPathStringAll` trims items, skips empty ones, and joins with `', '` by default.

**Windows assumptions in upstream modules:** `lua/utils/nodejs.lua` runs `cmd.exe /c node`, and `cloudflare.lua` runs `python lua\websitebypass\cloudflare.py`. `fmd.subprocess` translates these on Linux: `cmd.exe /c X` becomes a direct exec of X, and backslashes become slashes. The Docker image ships `python3` and `node`, and compose adds a FlareSolverr sidecar with `websitebypass_config.json` pointing at it.

## Data (fmd-store)

**`app.db`** has these tables:
- `tasks`, `task_chapters` (per-chapter status), `task_pages` (status + url). These replace FMD2's newline-joined columns.
- `favorites`, `downloaded_chapters`, `module_settings` (options, HTTP overrides, limits, cookies), `accounts` (encrypted at rest).
- `settings` (key → JSON), `events` (inbox and history), `module_files` (synced Lua files: path, sha, last_modified).

**`lists.db`** holds a single `masterlist(module_id, link, title, alttitles, authors, artists, genres, status, summary, numchapter, added_jdn)` table with FTS5. One table avoids FMD2's limit of 125 `ATTACH`ed site DBs. FMD2-DB prebuilt `<site>.7z` files are downloaded, extracted (`sevenz-rust`) and bulk-imported into it.

The importer reads FMD2's schemas (documented from `DownloadsDB.pas`, `FavoritesDB.pas`, `DownloadedChaptersDB.pas`, `DBDataProcess.pas`) and `modules.json`. Account passwords are decrypted with the `EncryptString` key from `uBaseUnit.pas:1556-1587`.

## Download engine (fmd-core, mirrors `uDownloadsManager.pas`)

**Statuses:** Stopped, Waiting, Preparing, Downloading, Converting, Compressing, Finished, Failed, Disabled.

**Per-chapter pipeline:**
1. `OnGetPageNumber`.
2. Skip pages and archives that already exist.
3. `OnGetImageURL`, unless `DynamicPageLink` is set.
4. Download page indices in parallel workers: `OnBeforeDownloadImage`, then `OnDownloadImage` or a plain GET, then `OnSaveImage` or a built-in save, then `OnAfterImageSaved`.
5. Convert images, then pack.

**Recovery:** retry failed chapters N times. On startup, resume tasks that were Downloading, Preparing or Waiting.

**Naming:** templates `%MANGA% %CHAPTER% %NUMBERING% %WEBSITE% %AUTHOR% %ARTIST% %FILENAME%`, using FMD2's `CustomRename` rules. Stripping illegal filename characters becomes configurable, with a POSIX default.

**Image conversion:** PNG→JPEG and WebP→PNG/JPEG built in. ImageMagick is optional and called as `magick` on PATH.

## Background jobs
- **Favorites check:** at startup and on an interval. Diff against `downloaded_chapters`, then auto-download or post an inbox item.
- **Update lists:** directory count, then name-and-link pages (stop early when the module sets `SortedList`), then optional info.
- **Module updater:** GitHub `commits?sha=master&per_page=1` with an ETag, then the `git/trees/{sha}:lua?recursive=1` tree; download changed blobs from raw.githubusercontent.com. Then **hot-reload**: invalidate the cached module bytecode and rebuild worker states when idle. Report modules that fail `Init` or call unknown Host API functions as inbox items instead of crashing.

## Web (SvelteKit SPA, variant B)
- **Library:** cover grid of favorites with filter chips.
- **Series page:** info, chapter list, download box.
- **Discover:** website picker, facets, tri-state genre filter, search over FTS.
- **Queue:** grouped by status, with history filter and speed graph.
- **Settings:** single page with a table of contents.
- **System:** logs, about, background jobs.
- **Shared chrome:** inbox popover, add-by-URL field, bottom queue dock.
- **Live updates:** one SSE stream (`/api/events`) for task progress, jobs, inbox items and log lines.
- **Covers:** served through `/api/covers/...` (fetched with the module's cookies and referer, then cached on disk).
- **Completed downloads:** the "Get files" button streams the CBZ or archive.

## Ticket list

Each ticket has a spec in `docs/tickets/T<nn>-<slug>.md`; T00 (this documentation) has none.

"Deps" lists the tickets that must be finished first. Tickets with no dependency between them can run at the same time; the waves below group them.

### Foundation
| # | Ticket | Deps |
|---|---|---|
| T01 | Cargo workspace, crate stubs, CI (fmt, clippy, test), dev CLI skeleton | — |
| T02 | Upstream Lua sync script (fetch `lua/` from dazedcat19/FMD2 into a fixture dir) + module corpus test harness | T01 |

### Lua runtime (the core)
| # | Ticket | Deps |
|---|---|---|
| T03 | `LuaClass` binding helper: dot and colon calls, `__get`/`__set` properties, default array property, silently ignored unknown keys, `.self()` | T01 |
| T04 | TStrings (`fmd.strings`, 0-based default index) and MemoryStream objects | T03 |
| T05 | Global helpers: `print`, `sleep`, `Trim`, `MaybeFillHost`, `MangaInfoStatusIfPos`, `GetBetween`, `SeparateLeft`/`SeparateRight` | T03 |
| T06 | Module loader: package searcher, `fmd.env`, `NewWebsiteModule`/`Init` scan, `MODULE` object (properties, AddOption*, Storage, Guardian, Account), `ModuleDef` registry | T03, T04 |
| T07 | XPath FPC shim: `xpath-fpc` library exporting internettools over a C ABI, built in CI | T01 |
| T08 | `fmd-xpath` trait, FFI backend, and the `CreateTXQuery`/`IXQValue` Lua bindings | T03, T07 |
| T09 | `fmd-http` client with Synapse semantics: redirects, retries, decompression, cookie jar, proxy, per-module connection queue | T01 |
| T10 | `HTTP` Lua object (Headers swap, `GET` returning true on a non-empty body, POST MimeType rules) | T04, T09 |
| T11 | `fmd.crypto`, most-used functions first; X25519 and SecretStream last | T03 |
| T12 | `fmd.duktape.ExecJS` via rquickjs, with CommonJS `require` from `lua/` | T03 |
| T13 | Remaining libs: gzip, fileutil, logger, subprocess (with Windows command translation), imagepuzzle, mangafoxwatermark, and the `pb` C module | T03 |
| T14 | Callback runner and worker pool: one Lua state per thread, cached per module; sets the MANGAINFO/TASK/UPDATELIST globals; GC every 16 calls | T06, T08, T10 |
| T15 | Dev CLI `module init|info|pages` with HTTP `--record`/replay | T14 |
| T16 | Smoke list of ~30 sites, recorded fixtures, replay in CI, nightly live run | T15 |

### Backend
| # | Ticket | Deps |
|---|---|---|
| T17 | `fmd-store`: `app.db` and `lists.db` schemas, migrations, repositories | T01 |
| T18 | Settings model: typed settings, defaults matching FMD2, per-module overrides | T17 |
| T19 | `fmd-pack`: folder/zip/cbz/pdf/epub, image conversion, filename templates | T01 |
| T20 | Download engine: task state machine, per-chapter pipeline, page workers, retries, resume on startup | T14, T17, T19 |
| T21 | `fmd-server`: axum, OpenAPI, SSE `/api/events`, optional auth, embedded SPA | T17 |
| T28 | Cover proxy and disk cache | T10, T21 |
| T29 | Module updater: GitHub ETag/tree sync, hot reload, Init failures and unknown-API reports to the inbox | T14, T17 |
| T30 | Anti-bot: post-request hook, websitebypass flow, cookie and user-agent persistence, FlareSolverr in compose | T10, T12, T13 |
| T31 | Accounts and login (`OnLogin`/`OnAccountState`), credentials encrypted at rest | T14, T17 |
| T32 | FMD2 importer: downloads, favorites, downloaded chapters, `modules.json`, settings | T11, T17 |
| T33 | Docker image (python3, node, magick) and release builds | T21, T30 |

### Frontend (SvelteKit, variant B)
| # | Ticket | Deps |
|---|---|---|
| T22 | App skeleton: routing, design tokens from the prototype, top nav, inbox popover, queue dock, SSE store; runs against a mock API until T21 lands | T01 |
| T23 | Queue: API and page (status groups, history filter, speed graph, task actions, "Get files") | T20, T21, T22 |
| T24 | Series page and add-by-URL (manga info, chapter list, download box) | T14, T21, T22 |
| T25 | Library: favorites API, grid, new-chapter check job, auto-download | T20, T24 |
| T26 | Discover: list update job, FMD2-DB 7z import, FTS search, facets and tri-state genres | T14, T17, T22 |
| T27 | Settings page, including per-module options and limits | T06, T18, T22 |
| T36 | System page: logs, background jobs, about | T21, T22 |

### Native XPath (last)
| # | Ticket | Deps |
|---|---|---|
| T34 | Native Rust XPath backend: xee, html5ever, and Xidel extensions (`json()`, dot-path into JSON, `?*`, `jn:*`, `css()`) | T08 |
| T35 | XPath differential corpus from smoke runs; switch the default backend at full parity | T16, T34 |

### Parallel waves
- **Wave 1** (after T01), six independent lanes:
  - Lua binding core: T03
  - XPath shim: T07
  - HTTP client: T09
  - Storage: T17
  - Packing: T19
  - Frontend skeleton on mocks: T22

  T02 can also run here.
- **Wave 2:** once T03 is done, T04, T05, T11, T12 and T13 fan out in parallel. Also in this wave: T08 (needs T03 and T07), T10 (needs T04 and T09), T18, and T21.
- **Wave 3:** T06, then T14. T14 is the critical path, because almost every feature waits on it.
- **Wave 4** (after T14): T15/T16, T20, T24, T26, T27, T29, T30, T31, T32 and T28 are largely independent of each other.
- **Wave 5:** T23, T25, T33 and T36, then T34 and T35. T34 can start any time after T08 as a side lane.

**Critical path:** T01 → T03 → T04 → T06 → T14 (also needs T07→T08 and T09→T10) → T20 → T23/T25. Put the most attention on T03, T07 and T09, because they unblock everything else.

**Milestone checkpoints:**
- **M1 "modules run"**: T01–T16. Every module passes Init, and the smoke list passes.
- **M2 "downloads work"**: T17–T20.
- **M3 "usable UI"**: T21–T24 and T27.
- **M4 "parity"**: everything else.

## Verification
- **Host API unit tests:** Rust tests that run small Lua snippets against mocked HTTP. They cover each quirk above: dot and colon calls, ignored unknown properties, 0-based TStrings, `GET` returning true on a 404 with a body, Headers swapping, and redirects.
- **Module corpus test (CI):** load every module in `lua/modules` and assert that `Init` succeeds. Statically scan the modules for Host API names that are not implemented.
- **Record/replay smoke tests:** a `--record` mode saves HTTP exchanges for the smoke list. CI replays them offline and checks that `OnGetInfo` and `OnGetPageNumber` give stable outputs. A nightly live run reports module breakage separately from regressions in FMD2r itself.
- **XPath differential tests:** the FPC backend logs (expression, document hash) pairs during smoke runs, building a corpus. The native backend must produce identical results on all of them before it becomes the default.
- **End to end:** run `fmd2r serve` (or `docker compose up` with FlareSolverr), add a MangaDex URL from a phone browser, add it to the library, download 2 chapters as CBZ, then check the file contents, the queue SSE updates, and that progress survives a restart.
