# Hooks

References point at upstream FMD2 `ad3a5b63`. The state and object mechanics are described in [runtime.md](runtime.md).

## 1. How a hook call works

A module registers a hook by assigning a **global function name** to the module property in `Init`, for example `m.OnGetInfo = 'GetInfo'`. During the loader's post-Init step (`LuaWebsiteModules.pas:554-583`), each non-empty name wires a Pascal dispatcher `DoX` into the `TModuleContainer`. The host only calls hooks that were wired.

Each dispatcher (`LuaWebsiteModules.pas:154-465`) does the following:

1. `L := GetLuaWebsiteModuleHandler(module)` gets this thread's state, rebuilt if the module changed. This also sets `MODULE` ([runtime.md §2.2](runtime.md#22-worker-states-one-per-os-thread-rebuilt-on-module-switch)).
2. It sets the per-hook globals listed below, using `LoadObject` for objects and plain `setglobal` for scalars.
3. `L.CallFunction(name)` looks up the global at call time and calls it as `lua_pcall(…, 0 args, LUA_MULTRET)`.
4. It reads the result from **stack index -1**, which is the **last** returned value (`return a, b` yields `b`). The reader depends on the hook's type:

   | Hook type | Reader | What a missing return gives |
   |---|---|---|
   | byte hooks | `lua_tointeger(-1)` | `nil`, `true` and non-numeric values give `0`, which is `NO_ERROR` |
   | boolean hooks | `lua_toboolean(-1)` | `nil` gives `false` |
   | `OnSaveImage` | `luaToString(-1)` | `nil` gives `''` |

5. Any exception (Lua error, missing function, host exception) is logged as `LUA>DoX("file.lua")>…` and the dispatcher returns its default: `INFORMATION_NOT_FOUND`, `False` or `''`.

Every hook receives **no arguments**. All inputs are globals. `MODULE` is always set. `HTTP` is the `THTTPSendThread` the host prepared with `module.CreateHTTP(thread)` / `PrepareHTTP` (`WebsiteModules.pas:390-420`). That preparation installs the module cookie manager, the connection queue, the anti-bot `OnHTTPRequest` wrapper, settings cookies, the user agent and the proxy.

### Status constants

`LuaPushNetStatus` sets `no_error=0`, `net_problem=1` and `information_not_found=2` (`LuaWebsiteModules.pas:825-830`, values from `WebsiteModules.pas:17-20`).

`LuaPushAccountStatus` sets `asUnknown=0`, `asChecking=1`, `asValid=2` and `asInvalid=3` (`:832-838`, enum `TAccountStatus` in `WebsiteModules.pas:76`).

Both sets are **pushed only by the hooks marked below**. Once pushed, they stay in the state as plain globals. A hook that doesn't push them, such as `GetPageNumber`, still sees them if an earlier hook on the same state did. Module code that returns `no_error` from `GetPageNumber` usually works only because of this.

| Constant | Uses |
|---|---|
| `no_error` | 1876 uses in 664 files |
| `net_problem` | 915 uses in 324 files |
| `information_not_found` | 0 uses |
| `asUnknown` / `asChecking` / `asValid` / `asInvalid` | 15 / 12 / 13 / 12 uses in 12 files |

FMD2r should define all of them in every worker state up front. That is a superset of upstream behaviour and removes the order dependency.

## 2. Hook reference

"Defined in" counts Init assignments of the hook property across `lua/` (see [usage-counts.md](usage-counts.md)).

### 2.1 Directory and list update hooks (Update manga list)

The driver is `TUpdateListManagerThread.Execute` (`uUpdateThread.pas:626-…`). For each selected module it runs:

1. `OnAfterUpdateList`, then `OnBeforeUpdateList` (**in that order**, `:671-674`).
2. Directory count phase:
   * If `Settings.UpdateListDirectoryPageNumber > 0`, it is used as the page count and the list is walked inverted.
   * Otherwise `CheckOut(TotalDirectory, CS_DIRECTORY_COUNT)` calls `OnGetDirectoryPageNumber` once per directory index, from worker threads.
3. Names phase: for each directory `j`, it sets `MODULE.CurrentDirectoryIndex = j` and calls `OnGetNameAndLink` once per page `0 … TotalDirectoryPage[j]-1`, from worker threads.
4. `OnBeforeUpdateList` again (`:708-709`).
5. `OnGetInfo` for each newly found title, unless the update-without-info option is set.
6. On termination it calls `OnAfterUpdateList` (`:685-686`).

| Hook | Defined in | Globals set | Return contract | Host behaviour |
|---|---|---|---|---|
| `OnBeforeUpdateList` | 1 | `UPDATELIST` (the global `updateList` manager object) | boolean, **ignored** | Called as described above. Runs on the update manager thread. |
| `OnAfterUpdateList` | 0 | `UPDATELIST` | boolean, **ignored** | Same. |
| `OnGetDirectoryPageNumber` | 235 | net status, `HTTP`, `UPDATELIST`, `PAGENUMBER` (integer, preset to 1), `WORKPTR` (0-based directory index; never read by any module) | byte, **ignored** (`uUpdateThread.pas:199`) | After the call the host reads global `PAGENUMBER`. If it isn't nil, the host stores it as the page count for that directory, clamped to ≥ 1. Modules write `PAGENUMBER` (161 writes, 152 files). `HTTP.Reset()` runs afterwards. |
| `OnGetNameAndLink` | 617 | net status, `HTTP`, `NAMES` (TStringList), `LINKS` (TStringList), `UPDATELIST`, `URL` = **page index as a decimal string, 0-based** (`IntToStr(FWorkPtr)`, `uUpdateThread.pas:234`; reversed when inverted) | byte | If the result ≠ `INFORMATION_NOT_FOUND` and `LINKS.Count > 0`, the host removes the host part from each link (`RemoveHostFromURLsPair`) and adds the pairs `(NAMES[i], LINKS[i])` to the list DB. `NAMES` must be at least as long as `LINKS`. With `SortedList = true`, a link already in the DB stops further paging. Afterwards the host runs `HTTP.Reset()`, `NAMES.Clear()` and `LINKS.Clear()` (`:266-268`). A module may grow the page count during this phase with `UPDATELIST.CurrentDirectoryPageNumber = n` ([objects.md §8](objects.md#8-updatelist--tupdatelistmanagerthread)). |

### 2.2 Manga information

| Hook | Defined in | Globals set | Return contract | Host behaviour |
|---|---|---|---|---|
| `OnGetInfo` | 623 | net status, `MANGAINFO` (TMangaInfo), `HTTP`, `URL` (the stored manga link, usually a path) | byte: `no_error` / `net_problem` / `information_not_found` | See below. |

Before the `OnGetInfo` call, `TMangaInformation.GetInfoFromURL` (`uData.pas:85-…`):

* returns `INFORMATION_NOT_FOUND` if `URL` is blank;
* clears `CoverLink`, `ChapterNames` and `ChapterLinks`;
* presets `MANGAINFO.URL = FillHost(RootURL, URL)`.

After the call it post-processes the result (`uData.pas:112-185`):

* `Link` defaults to `URL` without its host.
* `Title` is cleaned, and an empty title becomes `'N/A'`.
* Author, artist and genre lists are trimmed of trailing commas.
* The two chapter lists are aligned to the same length and trimmed.
* Duplicate chapter links are removed (case-insensitive).
* The host is stripped from chapter links.
* Chapter names are cleaned.

Callers:

* the manga-info view (`uGetMangaInfosThread.pas:93`);
* the update list (`uUpdateThread.pas` `GetInfo`);
* the favorites check (`uFavoritesManager.pas:346,417`);
* silent add-to-download/favorites (`uSilentThread.pas:495`);
* Check Modules ([§3](#3-onchecksite-and-mangacheck-debug-check-modules-form)).

### 2.3 Download pipeline

`TTaskThread.Execute` (`uDownloadsManager.pas:975-…`) runs these steps for each chapter `CurrentDownloadChapterPtr` that isn't already done:

1. `OnTaskStart`.
2. If `PageLinks` is empty, `OnGetPageNumber`.
3. `CheckForExists`: existing files mark the page `'D'`, the rest are reset to `'W'` (or `'G'` when `DynamicPageLink`).
4. If no page links exist, the host adds a single `'W'`.
5. `PageNumber := PageLinks.Count`.
6. If `not DynamicPageLink` and any link is `'W'` or `''`, the page-link phase runs `OnGetImageURL` per page in parallel threads.
7. The download phase runs the image hooks per page in parallel threads.
8. Convert and compress.

Each per-page thread (`TDownloadThread`) owns its own `HTTP` and runs `HTTP.Reset()` between pages (`:420-445`). Page-link placeholders are `'W'` (waiting), `'D'` (done) and `'G'` (dynamic).

| Hook | Defined in | Globals set | Return contract | Host behaviour |
|---|---|---|---|---|
| `OnTaskStart` | 3 | `TASK` (TTaskContainer) | boolean, **ignored** | Called before every chapter (`:1163-1166`). |
| `OnGetPageNumber` | 623 | `TASK`, `HTTP`, `URL` = `TASK.ChapterLinks[TASK.CurrentDownloadChapterPtr]` (as stored, usually a path) | boolean. It only affects the progress counter (`:829-860`). | The module fills `TASK.PageLinks` (image URLs) and/or sets `TASK.PageNumber`. If `PageNumber > PageLinks.Count`, the list is padded with `'W'`, and those slots are later filled by `OnGetImageURL`. Page links are trimmed. **No net status is pushed** (see [§1](#status-constants)). |
| `OnGetImageURL` | 40 | `TASK`, `HTTP`, `WORKID` (0-based page index), `URL` = current chapter link | boolean (true counts as success) | Called once per page whose link is `'W'` or `''` (`TDownloadThread.DoPageLink`, `:421-432`). The module writes `TASK.PageLinks[WORKID] = imageURL`. Links that are still empty become `'W'`. |
| `OnBeforeDownloadImage` | 114 | `TASK`, `HTTP`, `WORKID`, `URL` = page link | boolean, **ignored**: overwritten by the next step (`:378-383`) | Called after the host runs `HTTP.Reset()` and `HTTP.AcceptImage` (`Accept: image/webp,*/*`). It is typically used to set `Referer` and other headers. The Pascal side takes `var AURL`, but the Lua `URL` is **not written back**. |
| `OnDownloadImage` | 12 | `TASK`, `HTTP`, `WORKID`, `URL` | boolean (true means success) | Replaces the host's own `HTTP.GET(URL)`. If `TASK.PageNumber == TASK.PageContainerLinks.Count` and `WORKID` is in range, `URL` is `PageContainerLinks[WORKID]` instead of the page link (`:372-375`). On success the host saves `HTTP.Document` unless the image file already exists. |
| `OnSaveImage` | 0 | `HTTP`, `PATH` (chapter directory), `FILENAME` (target base name without extension) | string: the saved file path (`''` means failure) | Replaces `SaveImageStreamToFile(HTTP, PATH, FILENAME)`, which saves `HTTP.Document` with an extension sniffed from the content and stamps `Last-Modified`. Unused. |
| `OnAfterImageSaved` | 1 | `FILENAME` = full saved path | boolean: **becomes the page result** | Called after the page is marked `'D'` (`:405-410`). |

Image filenames come from `TASK.FileNames[WORKID]` when `FileNames.Count == PageLinks.Count`, otherwise from `%.3d` of `WORKID+1`, combined with the custom rename pattern (`TTaskThread.GetFileName`, `uDownloadsManager.pas:530-551`).

### 2.4 Accounts

| Hook | Defined in | Globals set | Return contract | Host behaviour |
|---|---|---|---|---|
| `OnLogin` | 28 | account status, `HTTP` (a fresh module HTTP) | boolean, **ignored** by its only host caller | The account-manager check thread (`frmAccountManager.pas:126-131`) sets `Account.Status = asChecking`, calls `OnLogin`, then repaints. The module sets `MODULE.Account.Status` itself. Modules also call their own login function from other hooks. |
| `OnAccountState` | 2 | account status | boolean, ignored | Called on the GUI thread when the user enables or disables the account (`frmAccountManager.pas:299`). |

## 3. `OnCheckSite` and `MANGACHECK` (debug "Check Modules" form)

`OnCheckSite` is defined in **1 file** (`modules/Atsumaru.lua`) and `MANGACHECK` has 5 uses in that file.

The loader wires `Module.OnCheckSite := @DoCheckSite`. `DoCheckSite` pushes the account status and nothing else (`LuaWebsiteModules.pas:449-465`), and **no host code calls it**. The real caller is the debug-only **Check Modules** form, which reaches into the `TLuaWebsiteModule` and calls the function by name.

### Scan (`TModuleScanThread.Execute`, `frmCheckModules.pas:171-300`)

For every module whose `OnCheckSite` dispatcher is assigned:

1. `L := GetLuaWebsiteModuleHandler(module)` and `LuaPushNetStatus`.
2. `LoadObject('MANGACHECK', TMangaCheck, luaMangaCheckAddMetaTable)`. The fields start as `MangaURLAddRootHost = true`, `ChapterURLAddRootHost = true` and `ChapterURLPrefix = ''` (`uBaseUnit.pas:3036-3042`).
3. `L.CallFunction(OnCheckSite)`. The return value is ignored. The module fills `MANGACHECK.MangaURL`, `MangaTitle`, `ChapterURL`, `ChapterTitle` and optionally `ChapterURLPrefix` and the two `*AddRootHost` flags.
4. Host normalisation:
   * `MangaURL` gets `MaybeFillHost(RootURL, …)` when `MangaURLAddRootHost` is set.
   * `ChapterURL` gets `MaybeFillPrefix(ChapterURLPrefix, …)`, which inserts the prefix after the host unless it's already present. Without a prefix, a leading `/` is added when the URL doesn't start with `http`. Then `MaybeFillHost(RootURL, …)` is applied when `ChapterURLAddRootHost` is set.
   * `TestToCheck` counts the tests that apply.
5. Modules that end up with a manga URL or a chapter URL are listed for testing.

### Test (`TModuleCheckThread.Execute`, `:351-…`)

`TestGetInfo` (`:1063-1181`):

* Creates a fresh `HTTP` with `CreateHTTP()`, sets `URL = MangaURL` and the net status, loads `MANGAINFO` and `HTTP`, and calls `OnGetInfo` directly. It does **not** go through `GetInfoFromURL`, so no post-processing happens.
* `lua_tointeger(-1) ≠ 0` is reported as a "Runtime error".
* Otherwise it requires a non-empty `Title` and `ChapterLinks`, and normalises the chapter links as above.
* It compares against `MangaTitle`, then checks that `ChapterURL` is among the chapter links (via `IndexOf`) and compares `ChapterTitle`.
* If `ChapterURL` was empty, it takes the first chapter link.

`TestGetPageNumber` (`:1183-1247`):

* Uses a fresh `HTTP`, `URL = ChapterURL` and a **fresh `TTaskContainer`** with no `TaskThread`. Reading `TASK.CurrentMaxFileNameLength` there would dereference nil.
* Calls `OnGetPageNumber` and requires `PageLinks.Count > 0`.

The `MANGACHECK` object is specified in [objects.md §9](objects.md#9-mangacheck--tmangacheck).

## 4. Hook-name properties without a dispatcher at runtime

All 15 `On*` properties are plain string properties on the module. Each holds only a name. Assigning one after Init (`MODULE.OnGetInfo = …` inside a hook) changes the name the next call looks up, but **does not wire or unwire a dispatcher**: wiring happens once, after Init. No module does this (`MODULE.On*` has 0 writes).

## 5. Anti-bot hooks (`websitebypass/`)

These two hooks are not module hooks. They are fixed global function names in two host-run scripts (`LuaWebsiteBypass.pas`). The anti-bot ticket (#6) covers the behaviour. The host side is:

* **Wrapping.** Every `HTTP.GET`/`POST`/`HEAD`/`XHR` on a module HTTP object goes through `TModuleContainer.WebsiteBypassHTTPRequest` → `WebsiteBypassRequest` (`LuaWebsiteBypass.pas:142-209`). `HTTP.Request` does **not**: it calls `HTTPRequest` directly. If `checkantibot.lua` or `websitebypass.lua` is missing, the wrapper just performs the request.
* **The request itself.** The wrapper sets `AHTTP.AllowServerErrorResponse := True` before the request, so 5xx responses return immediately without retries.
* **`____CheckAntiBot(HTTP)`** (`checkantibot.lua`) runs in the single process-wide check state, under a global mutex, after **every** wrapped request.
  * The `HTTP` argument is a fresh userdata for the request's HTTP object. This is the only hook that gets an argument.
  * It must return one boolean (read from stack slot 1).
  * Upstream returns true for status 403, 429 or 503 with an HTML content type and a `Server:` header containing `cloudflare` or `ddos-guard`.
* **`____WebsiteBypass(METHOD, URL)`** (`websitebypass.lua`) runs when the check returns true and the module's bypass mutex is free (`TryEnter`). Otherwise the request is simply retried, unless the thread is terminated.
  * It runs inside the **calling thread's worker state** (`HTTP.LuaHandler`), or in a temporary `TLuaHandler` if this HTTP object was never loaded into a state. In the temporary case `MODULE` is unset.
  * The host first sets global `HTTP`, clears `module.Settings.HTTP.Cookies` for the module found by host (`Modules.LocateModuleByHost`), and runs the chunk once per state.
  * It calls the function with two string arguments and reads a boolean from stack slot 1.
  * On true: `Settings.Enabled := True`, `Settings.HTTP.Cookies := HTTP.Cookies.Text` (CRLF replaced with `;`), and `Settings.HTTP.UserAgent := HTTP.UserAgent`.
  * If the module's `Storage['reload']` contains `true`, the host runs `HTTP.Reset()` and repeats the original request.
* **Response.** The wrapper finally copies `HTTP.Document` into the optional `Response` object.
* **What the bypass scripts use:**
  * `require 'websitebypass.cloudflare'` / `'websitebypass.ddos-guard'`;
  * `fmd.logger`, `fmd.env`, `fmd.duktape`, `fmd.crypto`, `fmd.subprocess` (they run Python/FlareSolverr through `RunCommandHide`), `utils.json`;
  * `io.open` on `lua\websitebypass\websitebypass_config.json`, a relative Windows path;
  * `MODULE.Storage['reload']`;
  * `HTTP.Request`, `HTTP.ClearCookiesStorage`, `HTTP.ParseServerCookies`, `HTTP.EnabledCookies`, `HTTP.RetryCount`;
  * the ignored setter `HTTP.FollowRedirection`.
