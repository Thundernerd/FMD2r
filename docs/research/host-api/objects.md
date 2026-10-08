# Host objects

References point at upstream FMD2 `ad3a5b63`. Every object follows the convention in [runtime.md §5](runtime.md#5-the-userdata-calling-convention-luaclasspas): methods are **dot-called**, unknown keys are ignored on write and return the metatable on read, and argument coercion follows [runtime.md §5.5](runtime.md#55-type-coercion-at-the-boundary).

## Table format

* **Kind:**
  * `prop rw` / `prop ro`: a property with getter (and setter);
  * `field`: a field-bound property, always read-write;
  * `method`;
  * `object`: a child userdata, created once per push;
  * `array`: an indexed property proxy (`obj.X[k]`, `obj.X[k] = v`);
  * `default[]`: `obj[k]` / `obj[k] = v` through `__defaultget`/`__defaultset`.
* **Uses** is `occurrences / files` across `lua/` (method in [usage-counts.md](usage-counts.md#method)).
  * For member names that several classes share (`Get`, `Count`, `ToString`, `Add`, `Clear`), the counts say which receiver was counted.
  * `0` means no use anywhere in `lua/`.

---

## 1. `MODULE` / `TLuaWebsiteModule`

Source: `luaWebsiteModuleAddMetaTable`, `LuaWebsiteModules.pas:991-1040`, with methods at `:848-975`.

`MODULE` is set as a global in worker states. It is the value `NewWebsiteModule()` returns in Init (usually held in local `m`).

`MODULE.x` uses are listed first. `m.x =` writes in Init are listed after `+`.

| Member | Kind | Type | Semantics | Uses |
|---|---|---|---|---|
| `ID` | field | string | Unique module id, usually a 32-character hex string. Modules with an empty `ID` or `Name` are dropped after Init. `LocateModule` binary-searches the list sorted by `ID`. | 17 + 623 writes |
| `Name` | field | string | Display name. | 11 + 623 |
| `RootURL` | field | string | Base URL. **Lower-cased** by the host after Init. Used by `LocateModuleByHost` (substring match) and `MaybeFillHost`. | 1300 reads + 623 |
| `Category` | field | string | UI grouping, such as `English` or `Indonesian`. | 619 |
| `MaxTaskLimit` | field | int | Maximum concurrent tasks (0 means unlimited). The website settings override can replace it. | 18 |
| `MaxThreadPerTaskLimit` | field | int | Maximum download threads per task. | 1 |
| `MaxConnectionLimit` | field | int | Bound to `ConnectionsQueue.MaxConnections`. Caps concurrent HTTP requests per module across all threads (`THTTPQueue`). | 18 |
| `ActiveTaskCount` | field | int | Live counter. | 2 reads |
| `ActiveConnectionCount` | field | int | Live counter (`ConnectionsQueue.ActiveConnections`). | 0 |
| `SortedList` | field | bool | The update list stops paging at the first known link. | 128 |
| `InformationAvailable` | field | bool | Default true. | 0 |
| `FavoriteAvailable` | field | bool | Default true. | 0 |
| `DynamicPageLink` | field | bool | Skips the `OnGetImageURL` phase. Pages are marked `'G'`. | 1 |
| `TotalDirectory` | prop rw | int | Number of directories (lists) for the update. The setter resizes `TotalDirectoryPage[]` and fills it with 1s. | 38 |
| `CurrentDirectoryIndex` | field | int | Directory index currently being listed. The host sets it. | 44 reads |
| `AccountSupport` | prop rw | bool | Setting it true creates `Account`. Setting it false frees `Account` but leaves a dangling pointer (upstream bug, never exercised). | 29 |
| `LastUpdated` | field | string | Free-text date shown in the UI. | 28 |
| `Tag` | field | int | Scratch field. | 0 |
| `OnBeforeUpdateList` … `OnCheckSite` (15 names) | field | string | Hook names, see [hooks.md](hooks.md). | per hook |
| `Guardian` | object | TCriticalSection | Module mutex ([§10](#10-tcriticalsection)). | 0 |
| `Storage` | object | TStringsStorage | Module key-value store ([§3](#3-storage--tstringsstorage)). | 85 / 20 |
| `Account` | object | TWebsiteModuleAccount | **Present only if the account existed when the metatable was built.** It exists in worker states, never in Init ([§2](#2-moduleaccount--twebsitemoduleaccount)). | 97 / 12 |
| `AddOptionCheckBox(name, caption, default)` | method | (string, string, bool) → none | Adds a per-module UI option. `name` is the lookup key. It is persisted in `modules.json` under `CleanOptionName(name)`, which keeps only `[A-Za-z0-9_]` and strips leading digits. Saved values overwrite defaults at load time (`WebsiteModules.pas:493-506`). | 68 / 62 |
| `AddOptionEdit(name, caption, default)` | method | (string, string, string) | Text option. | 8 / 8 |
| `AddOptionSpinEdit(name, caption, default)` | method | (string, string, int) | Integer option. | 2 / 2 |
| `AddOptionComboBox(name, caption, items, default)` | method | (string, string, string, int) | `items` is a newline-separated list (`'Main\nSecondary\nCompress'`). The value is the **0-based index**. | 23 / 18 |
| `GetOption(name)` | method | string → bool / string / int / nil | Returns the current option value by `name`, looked up case-insensitively in a sorted list. Returns `nil` for unknown names. | 70 / 41 |
| `AddServerCookies([url,] cookies)` | method | With 2 args: `(url, cookies)`. With 1 arg: `(cookies)`, url `''`. | Parses `Set-Cookie`-style strings separated by `\n` into the module cookie jar. Domain and path come from the url. **With no url the domain is `''`, and such cookies never match any request**, so the one-arg form is a functional no-op (`httpcookiemanager.pas:175-188, 233-246`). | 2 / 2 (both one-arg) |
| `GetServerCookies(domain[, name])` | method | → string | Lines of the form `name=value; domain=…; path=…[; expires=…][; secure][; httponly][; samesite=…]`, separated by CRLF. | 0 |
| `RemoveCookies(domain[, name])` | method | → none | Deletes matching cookies. Domain matching is exact and case-insensitive. | 1 (no args, so domain `''`) |
| `ClearCookies()` | method | → none | Empties the module cookie jar. | 4 / 3 |
| `self()` | method | → lightuserdata | Raw pointer. Present on every object. | 0 |

**Cookie jar.** There is one per module (`THTTPCookieManager`), shared by every HTTP object the module creates on every thread. It is persisted to `modules.json` (persistent cookies only). It fills each request's `Cookie` header by domain, path, `HttpOnly` and expiry matching (`SetCookies`, `:218-289`). Settings cookies are then merged on top (`MergeHTTPCookiesFromSetting`).

## 2. `MODULE.Account` / `TWebsiteModuleAccount`

Source: `LuaWebsiteModules.pas:977-989`, `WebsiteModules.pas:78-95`. Persisted in `modules.json`; the strings are encrypted with `EncryptString`.

| Member | Kind | Type | Uses |
|---|---|---|---|
| `Enabled` | field | bool | 13 / 11 |
| `Username` | field | string | 12 / 12 |
| `Password` | field | string | 12 / 12 |
| `Status` | field | int (`asUnknown=0`, `asChecking=1`, `asValid=2`, `asInvalid=3`) | 53 / 12 (52 writes) |
| `Cookies` | field | string | 7 / 2 |
| `Guardian` | object | TCriticalSection | 0 |

## 3. `Storage` / `TStringsStorage`

Source: `LuaStringsStorage.pas`. One store per module, shared by **all threads and states** of that module. It lives in memory only and is not persisted. Writes are mutex-guarded. Reads are not.

| Member | Kind | Semantics | Uses |
|---|---|---|---|
| `Storage[key]` | default[] get | `TStringList.Values[key]`. A missing key returns **`''`, not nil**. Name matching is case-insensitive. A key containing `=` cannot be found. | 87 / 20 (both directions) |
| `Storage[key] = v` | default[] set | `Values[key] := tostring(v)`. Setting `''` **deletes** the key. A number becomes its string form. `nil` becomes `''` and deletes. | (included above) |
| `Text` | prop rw | All entries as `name=value` lines. | 0 |
| `Remove(key)` | method | Deletes the key. | 0 |
| `Free()` / `Destroy()` | method | **Frees the storage object.** Any later access is use-after-free. | 0 |
| `Tag` | field int | Default 0. | 0 |
| `Enable` | field bool | Default true. | 0 |
| `Status` | field string | Default `'my status of TStringsStorage'`. | 0 |

**Name shadowing.** The member names `Text`, `Remove`, `Free`, `Destroy`, `Tag`, `Enable`, `Status` and `self` win over `Storage[...]` keys with the same name. No module uses a colliding key; the keys used are listed in [usage-counts.md](usage-counts.md#5-storage-keys).

The anti-bot path reads `Storage['reload']` ([hooks.md §5](hooks.md#5-anti-bot-hooks-websitebypass)).

## 4. `HTTP` / `THTTPSendThread`

Source: bindings in `LuaHTTPSend.pas`. Behaviour is in `httpsendthread.pas` on top of Synapse `THTTPSend` (`baseunits/synapse/httpsend.pas`).

### 4.1 Members

| Member | Kind | Lua signature → result | Uses |
|---|---|---|---|
| `GET(url)` | method | → bool | 1183 / 328 |
| `POST(url[, data])` | method | → bool | 83 / 49 |
| `HEAD(url)` | method | → bool | 0 |
| `XHR(url)` | method | → bool | 4 / 4 |
| `Request(method, url)` | method | → bool (**no anti-bot wrapper**) | 6 / 2 |
| `Reset()` | method | → none | 129 / 96 |
| `ResetBasic()` | method | → none | 0 |
| `ClearCookies()` | method | → none | 0 on HTTP |
| `ClearCookiesStorage()` | method | → none | 1 (plus 2 via `http_m` in cloudflare) |
| `GetCookies()` | method | → string | 0 |
| `AddServerCookies([url,] cookies)` | method | → none | 0 on HTTP |
| `ParseServerCookies()` | method | → none | 1 |
| `SetProxy(type, host, port, user, pass)` | method | → none | 0 |
| `Terminated` | prop ro | bool | 15 / 13 |
| `LastURL` | prop ro | string | 4 / 2 |
| `ResultCode` | prop ro | int | 23 / 18 |
| `ResultString` | prop ro | string | 0 |
| `Headers` | object | TStrings, `NameValueSeparator=':'`, case-insensitive | 253 / 128 |
| `Cookies` | object | TStrings, `NameValueSeparator='='`, `Delimiter=';'` | 50 / 31 |
| `Document` | object | TMemoryStream (request and response body) | 1329 / 354 |
| `MimeType` | field | string (request `Content-Type`; replaced by the response `Content-Type`) | 42 / 24 |
| `UserAgent` | field | string | 20 / 6 |
| `RetryCount` | field | int | 7 / 3 |
| `EnabledCookies` | field | bool | 2 / 1 |

Assignments to the unbound names `FollowRedirection`, `Timeout`, `MaxRedirect` and `AllowServerErrorResponse` are **ignored**. `FollowRedirection` is assigned 2 times by `websitebypass/cloudflare.lua`.

### 4.2 Request semantics (`DefaultHTTPRequest`, `httpsendthread.pas:592-718`)

`GET`, `HEAD`, `POST` and `XHR` call `InternalHTTPRequest`, which goes through the anti-bot wrapper ([hooks.md §5](hooks.md#5-anti-bot-hooks-websitebypass)) and then `HTTPRequest`. `Request(method, url)` calls `HTTPRequest` directly. `HTTPRequest` then:

1. Returns `false` if the owning thread is terminated.
2. Waits for a slot in the module connection queue (`MaxConnectionLimit`).
3. Normalises the URL:
   * strips leading `:`, `/` and control/space characters and trailing whitespace;
   * runs `MaybeEncodeURL`, which URL-encodes the whole URL unless decoding would shorten it, meaning it isn't already encoded;
   * prepends `https://` if the URL has no `://`.

   So `'//host/x'` and `'host/x'` both become `https://host/x`.
4. **Auto-reset.** If `Headers.Text` starts with `HTTP/`, meaning the headers still hold the previous response, `Reset()` runs first. That clears the request headers, cookies and `Document`, so a POST body written into `Document` is lost too.
5. Normalises headers to `Name: Value`.
6. Sends:
   * Sets the `Cookie` header from the module jar, then merges settings cookies (skipped once after `ClearCookies()`).
   * Retries while the request fails, or (unless `AllowServerErrorResponse`) while `ResultCode > 500`. It stops after `RetryCount` retries; `RetryCount = -1` means retry forever. Request headers are restored before each retry.
   * After each request, if `EnabledCookies` is set it parses `Set-Cookie` into the jar. Otherwise it clears `Cookies`.
7. Follows redirects 301, 302, 303 and 307 (not 308), up to 5. Each redirect is a `GET` with a `Referer` added. A relative `Location` is resolved against the current host.
8. Decodes the body by `Content-Encoding`: `zstd`, `br`, or `gzip`/`deflate`. The gzip path (`GZIPUtils.unzipStream`) sniffs gzip, zlib and raw deflate.
9. **Return value: `Document.Size > 0`.** The status code plays no part. Network exceptions return `false`.
10. After a request:
    * `Headers` holds the **raw response headers**, and the first line is the status line, for example `HTTP/1.1 200 OK`. `Headers.Values['Content-Type']` keeps the space after the colon, so it reads `' text/html'`.
    * `ResultCode` and `ResultString` hold the status.
    * `LastURL` is the final URL after redirects.
    * `Document` holds the decoded body with position 0.
    * `MimeType` holds the response content type.

Method specifics:

* **`POST(url, data)`** (`:730-747`):
  * If `data ≠ ''`, it clears `Document` and writes `data` into it. Otherwise it sends the current `Document`.
  * It moves any `Content-Type` request header into `MimeType`.
  * `MimeType == 'text/html'` (the reset default) becomes `application/x-www-form-urlencoded; charset=UTF-8`.
* **`XHR(url)`** auto-resets like step 4, sets `X-Requested-With: XMLHttpRequest` and calls `GET`.

Resetting and the remaining methods:

* **`Reset()`** (`:924-932`) runs `ResetBasic` and then sets the default request headers: `DNT: 1`, `Upgrade-Insecure-Requests: 1`, `Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8`, `Accept-Language: en-US,en;q=0.5`, `Accept-Charset: utf-8`.
* **`ResetBasic()`** (`:934-946`) clears `Document`, `Headers` and `Cookies`, sets `MimeType='text/html'`, closes the socket and sets `Accept-Encoding: gzip, deflate, br, zstd`.
* **`ClearCookies()`** clears `Cookies` and skips cookie-jar injection for the next request only.
* **`ClearCookiesStorage()`** clears `Cookies` and the whole module jar.
* **`ParseServerCookies()`** parses `Set-Cookie` lines in the current `Headers` into the jar for `LastURL`.
* **`GetCookies()`** returns the `Cookies` list joined with `'; '`.
* **`SetProxy`** accepts types `HTTP`, `SOCKS4` and `SOCKS5`, case-insensitive. Any other type means no proxy.
* **`Terminated`** is true when the owning thread is terminating. Long module loops poll it.

The host-side defaults for a module HTTP object are `KeepAlive`, HTTP/1.1, the default user agent, `Compress=true`, `RetryCount` from settings, a 5-redirect limit and the settings proxy. The module's website settings can override the user agent and proxy (`PrepareHTTP`).

## 5. `TStrings` (`TStringList`)

Source: `LuaStrings.pas`. Host instances are `NAMES`, `LINKS`, `MANGAINFO.ChapterNames`/`ChapterLinks`, `TASK.PageLinks`/`ChapterLinks`/`ChapterNames`/`PageContainerLinks`/`FileNames`, `HTTP.Headers` and `HTTP.Cookies`. Lua-created instances come from `require 'fmd.strings'.Create()`, which no module uses.

Semantics are FPC `TStringList` with `Sorted=false` and **`CaseSensitive=false`**. Indexes are **0-based**.

| Member | Kind | Semantics | Uses on host lists |
|---|---|---|---|
| `Add(s)` | method | Appends `tostring(s)`. `nil` becomes `''`. Returns nothing. | 900 / 260 |
| `AddText(s)` | method | Splits `s` into lines (CR, LF, CRLF) and appends each. | 0 |
| `Get(i)` / `obj[i]` | method / default[] | Item `i`. **Out of range raises** (aborts the hook). A non-numeric key, such as `obj.foo`, reads as `Get(0)`. | `Get` 0, `[ ]` 82 / 47 |
| `Set(i, s)` / `obj[i] = s` | method / default[] | Replaces item `i`. Out of range raises. | 0, `[ ]=` 35 / 32 |
| `Strings[i]` | array | Same as `Get`/`Set`. | 0 |
| `Count` / `GetCount()` | prop ro / method | Number of items. | 61 / 42 (`Count`) |
| `Clear()` | method | Removes all items. | 39 / 37 |
| `Delete(i)` | method | Removes item `i`. Raises when out of range. | 1 |
| `IndexOf(s)` | method | First index of `s` compared **case-insensitively**, else `-1`. | 0 |
| `IndexOfName(n)` | method | First item whose name part (before the separator) equals `n`, case-insensitive. | 0 |
| `Values[n]` | array | Get: the value part of the first item named `n`, else `''`. Set: replaces it, or appends `n<sep>v`. **Setting `''` deletes the item.** | 306 / 139 |
| `Reverse()` | method | Reverses the order in place. | 413 / 205 |
| `Sort()` | method | Case-insensitive locale sort (`AnsiCompareText`). | 0 |
| `Text` / `GetText()` / `SetText(s)` | prop rw / methods | Get: every item followed by the line break, **CRLF** on upstream's Windows build, so a trailing CRLF is included. Set: clear, then split on CR, LF and CRLF. | 8 / 3 |
| `CommaText` | prop rw | FPC comma text: quoted with `"`, and the setter also splits on whitespace. | 1 |
| `DelimitedText` | prop rw | Uses `Delimiter` and `QuoteChar '"'`. `StrictDelimiter=false`, so whitespace also splits. | 0 |
| `Delimiter` | prop rw | One character. The setter takes the first character; `''` raises. | 0 |
| `NameValueSeparator` | prop rw | One character. Default `=`, but `:` for `HTTP.Headers`. | 0 |
| `LoadFromFile(path)` / `SaveToFile(path)` | method | File I/O. | 0 / 1 |
| `LoadFromStream(stream)` / `SaveToStream(stream)` | method | The argument must be a stream userdata, such as `HTTP.Document`. | 0 |

Host post-processing of these lists is described per hook in [hooks.md](hooks.md). Examples: `NAMES`/`LINKS` must align, and chapter lists are padded and trimmed.

## 6. `TMemoryStream` (`HTTP.Document`)

Source: `LuaMemoryStream.pas`.

| Member | Kind | Semantics | Uses |
|---|---|---|---|
| `ToString()` / `ReadString()` | method | Returns bytes **from the current position to the end**, then restores the position (`uBaseUnit.StreamToString`). After a request the position is 0. **After `WriteString` the position is at the end, so `ToString()` returns `''`.** | 627 / 170 (`ToString`, shared with IXQValue), 0 (`ReadString`) |
| `WriteString(s)` | method | Writes `s` at the current position. Binary-safe. Grows the stream. Does **not** truncate. Modules call `Document.Clear()` first or rely on a prior `Reset()`. | 6 / 6 |
| `LoadFromFile(path)` / `SaveToFile(path)` | method | Whole-stream file I/O. | 0 |
| `Clear()` | method | Size 0, position 0. | (in `Clear` count) |
| `Size` | prop rw | Byte size. | 0 |

Streams passed to `CreateTXQuery` and `ParseHTML` are read **from position 0**, whatever the current position (the local `StreamToString` in `XQueryEngineHTML.pas`). Host image saving writes the whole stream. `ImagePuzzle.DeScramble` belongs to the image ticket.

## 7. `MANGAINFO` / `TMangaInfo`

Source: `LuaMangaInfo.pas`.

| Member | Kind | Uses (reads + writes) |
|---|---|---|
| `URL` | field string. The host presets it to the full manga URL. | 237 / 128 (85 writes) |
| `Title` | field string | 404 / 298 |
| `AltTitles` | field string | 114 / 109 |
| `Link` | field string. The host derives it from `URL` when empty. | 0 |
| `CoverLink` | field string | 303 / 288 |
| `Authors` | field string, comma-separated | 229 / 219 |
| `Artists` | field string | 141 / 137 |
| `Genres` | field string, comma-separated | 295 / 261 |
| `Status` | field string. Convention: `'0'` completed, `'1'` ongoing, `'2'` hiatus, `'3'` cancelled, else unknown. Usually produced by `MangaInfoStatusIfPos`. | 222 / 220 |
| `Summary` | field string | 294 / 272 |
| `ChapterNames` | object TStrings | 546 / 306 |
| `ChapterLinks` | object TStrings | 551 / 306 |

`MANGAINFO.Artist` (1 write, `OrckuMangas.lua:91`) is a typo and is silently ignored.

## 8. `UPDATELIST` / `TUpdateListManagerThread`

Source: `LuaUpdateListManager.pas`, `uUpdateThread.pas:441-460, 614-623`.

| Member | Kind | Semantics | Uses |
|---|---|---|---|
| `CurrentDirectoryPageNumber` | prop rw int | The work limit of the current phase. In the names phase that is the page count of the current directory. The setter **ignores values lower than the current limit**, so the limit can only grow. Modules raise it from `OnGetNameAndLink` when they discover more pages. | 65 / 46 |
| `UpdateStatusText(s)` | method → none | Sets the status-bar text, prefixed with `Updating list [i/n] <module> \| [T:threads] [ptr/limit] `. | 29 / 26 |

## 9. `MANGACHECK` / `TMangaCheck`

Source: `LuaMangaCheck.pas`, `uBaseUnit.pas:350-368, 3036-3042`. Only set by the Check Modules scan ([hooks.md §3](hooks.md#3-onchecksite-and-mangacheck-debug-check-modules-form)).

| Member | Kind | Default | Uses |
|---|---|---|---|
| `MangaURL` | field string | `''` | 1 |
| `MangaTitle` | field string | `''` | 1 |
| `ChapterURL` | field string | `''` | 1 |
| `ChapterURLPrefix` | field string | `''` | 0 |
| `ChapterTitle` | field string | `''` | 1 |
| `MangaURLAddRootHost` | field bool | `true` | 0 |
| `ChapterURLAddRootHost` | field bool | `true` | 1 |

## 10. `TCriticalSection`

Source: `LuaCriticalSection.pas`. Exposed as `MODULE.Guardian` and `Account.Guardian`.

| Member | Kind | Uses |
|---|---|---|
| `TryEnter()` | method → bool | 0 |
| `Enter()` | method | 0 |
| `Leave()` | method | 0 |

## 11. `TXQuery` and `IXQValue` (method surface only)

Source: `LuaXQuery.pas`, `LuaIXQValue.pas`, `XQueryEngineHTML.pas`. XPath **semantics** belong to the sibling XPath ticket (#4/#8). This section covers how values cross the boundary.

**Error handling.** Every evaluation runs inside `TXQueryEngineHTML.Eval`, which wraps the engine call in `try … except end` and **returns an empty sequence on any error**: syntax errors, type errors, `json(*)` on non-JSON, and so on (`XQueryEngineHTML.pas` `Eval`). As a result:

* `XPathString` returns `''`;
* `XPathCount` returns `0`;
* the `*All` variants add nothing.

145 files rely on this through patterns like `tonumber(x.XPathString(…)) or 1` (173 occurrences). 539 calls in 144 files use `json(...)` expressions over the document. **Errors raised outside `Eval` are not swallowed** and abort the hook. Examples: `GetAttribute`/`InnerHTML`/`XPathHREFAll` on non-node items, where `toNode` is nil.

**`json("http://…")` / `doc(url)`.** Internet Tools' `json()` fetches a URL argument through `TXQStaticContext.retrieveFromURI` → `defaultInternet.get` (Internet Tools' own `internetaccess` client, compiled in via `{$DEFINE ALLOW_EXTERNAL_DOC_DOWNLOAD}` in `xquery.pas:46`). It **never uses FMD2's `HTTP` object**: no cookies, no headers, no anti-bot wrapper. FMD2 does not link any `internetaccess` backend unit (grep shows no `synapseinternetaccess`/`w32internetaccess`/`defaultInternetAccessClass` in FMD2 sources), so such a fetch is expected to fail. The `Eval` wrapper swallows that failure. **No file in `lua/` calls `json`, `doc`, `json-doc` or `unparsed-text` with a URL.** FMD2r can leave URL fetching out of XPath.

### `CreateTXQuery([html])` (global, `LuaXQuery.pas:22-43`)

| Argument | Result |
|---|---|
| Exactly one string or number | Parse it as HTML. |
| Exactly one userdata | Treated as a stream and read whole from position 0. |
| Anything else, including 0 or ≥ 2 args | An empty engine. |

Returns an auto-freed userdata.

Uses: 1159 / 347 in total. 1091 of them are `CreateTXQuery(HTTP.Document)` and 2 are `CreateTXQuery()`.

| TXQuery member | Lua signature → result | Semantics | Uses |
|---|---|---|---|
| `ParseHTML(html)` | string or stream → none | Re-parses. **An empty string is a no-op** and keeps the old tree. | 130 / 105 (51 with `HTTP.Document`) |
| `XPath(expr[, ctx])` | → IXQValue | `ctx` is an IXQValue (unchecked userdata). | 532 / 235 |
| `XPathString(expr[, ctx])` | → string | The result's `toString`. | 2242 / 330 |
| `XPathStringAll(expr)` | → string | Non-blank items, trimmed, joined with `', '`. | 569 / 267 in total |
| `XPathStringAll(expr, sep)` | → string | Same, joined with `sep` (any string or number argument). | |
| `XPathStringAll(expr, sep, ctx)` | → string | Same, in context `ctx`. | |
| `XPathStringAll(expr, list[, ctx])` | → none | `list.Add(trim(item))` for **every** item, blank ones included. | 133 / 122 with a host list |
| `XPathHREFAll(expr, links, texts[, ctx])` | → none | For each node: `links.Add(@href)` and `texts.Add(trim(string))`. | 196 / 130 |
| `XPathHREFTitleAll(expr, links, titles[, ctx])` | → none | For each node: `links.Add(@href)` and `titles.Add(@title)`. | 51 / 45 |
| `XPathCount(expr[, ctx])` | → int | Sequence length. | 17 / 15 |

The `CSS*` methods of the Pascal class are **not bound**.

| IXQValue member | Lua signature → result | Semantics | Uses |
|---|---|---|---|
| `Get()` | → iterator | For `for v in val.Get() do`. Yields each item as an IXQValue. An empty sequence yields nothing. | 346 / 191 |
| `Get(i)` | int → IXQValue | **1-based.** Out of range returns a wrapped empty or undefined value (a truthy userdata), not nil. | 120 / 48 |
| `Count` | prop ro int | Sequence length. A single value counts as 1. | (191 / 100 across all `Count`) |
| `ToString()` | → string | XPath string value. | (627 / 170 shared with streams) |
| `GetAttribute(name)` | → string | Node attribute, `''` if absent. **Raises on non-nodes.** | 159 / 119 |
| `GetProperty(name)` | → IXQValue | JSON object member. Empty for non-objects. | 336 / 76 |
| `InnerHTML()` / `OuterHTML()` / `InnerText()` | → string | Node only. | 0 / 0 / 0 |

## 12. `ImagePuzzle` (from `require 'fmd.imagepuzzle'`)

Source: `LuaImagePuzzle.pas`, algorithm in `ImagePuzzle.pas`. The algorithm belongs to the native-dependencies and image tickets.

`Create(hor, ver)` / `New(hor, ver)` returns an auto-freed puzzle. **It needs exactly 2 args.** With any other count it pushes nothing but still reports 1 result, so the caller gets the last argument back.

| Member | Kind | Uses |
|---|---|---|
| `Matrix[i]` | array int, 0-based tile map | 8 / 6 |
| `Flips[i]` | array int | 0 |
| `HorBlock` / `VerBlock` | prop ro int | 0 |
| `Multiply` | field int | 1 |
| `DeScramble(srcStream, dstStream)` | method | 8 / 6 |

Module pattern: `puzzle.DeScramble(HTTP.Document, HTTP.Document)`, or with a stream from another host object.

## 13. `TProcess` (from `require 'fmd.subprocess'`)

`Create()` / `New()` returns a `TProcess` with an **empty metatable**. It has no usable members (only `self`). It is unused. The library functions are covered in [globals-and-libraries.md](globals-and-libraries.md#fmdsubprocess-luasubprocesspas).
