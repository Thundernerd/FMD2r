# T10: `HTTP` Lua object
Deps: T04, T09

## Goal
Expose a `fmd-http` session to Lua as the global `HTTP` object with FMD2's exact surface: methods, properties, and the Headers/Document quirks modules depend on.

## Scope (in/out)
In:
- `HTTP` object built with the T03 helper over an `HttpSession`:
  - Methods (exactly the 13 in `LuaHTTPSend.pas:129-143`): `Request(method, url)`, `GET(url)`, `POST(url[, data])`, `HEAD(url)`, `XHR(url)`, `Reset()`, `ResetBasic()`, `ClearCookies()`, `ClearCookiesStorage()`, `GetCookies()`, `AddServerCookies(...)`, `ParseServerCookies(...)`, `SetProxy(type, host, port, user, pass)`.
  - Read-only properties (`:144-150`): `Terminated`, `LastURL`, `ResultCode`, `ResultString`.
  - Sub-objects (`:157-159`): `Headers` (TStrings), `Cookies` (TStrings), `Document` (MemoryStream), all from T04.
  - Read/write properties (`:160-163`): `MimeType`, `UserAgent`, `RetryCount`, `EnabledCookies`.
- Headers swap semantics visible from Lua: before a call `HTTP.Headers` holds request headers; after it holds the response headers; the next request resets if it starts with `HTTP/`.
- `GET` returns `true` on a non-empty body even for 404.
- `POST` MimeType rules.
- Module context: when running for a module, the session uses that module's cookie jar, connection queue and HTTP overrides (user agent, cookies from settings via `ModuleSettingsStore`).
- `CreateHTTP`-style factory for Rust callers (T14 sets the global per callback).

Out: anti-bot hook (T30); callback runner (T14).

## Seams under test
Lua snippets through the `fmd-lua` runtime with an `HTTP` global bound to a session whose transport is a local test server or a stub transport:
```lua
HTTP.Headers.Values['X-Test'] = '1'
assert(HTTP.GET('127.0.0.1:PORT/404-with-body') == true)   -- body non-empty
assert(HTTP.ResultCode == 404)
assert(HTTP.Headers.Values['Content-Type'] ~= '')          -- now response headers
assert(HTTP.Document.ToString() == 'nope')
HTTP.Reset()
HTTP.Headers.Values['Content-Type'] = 'text/html'
HTTP.POST(url, 'a=1')                                      -- server sees form-urlencoded
assert(HTTP:GET(url) == HTTP.GET(url))                     -- colon and dot calls
```

## Acceptance criteria
- [ ] Every method and property in `LuaHTTPSend.pas` exists with the same name and call style.
- [ ] Headers swap, GET-on-404, and POST MimeType behaviours tested from Lua.
- [ ] `Document` is binary-safe (an image body round-trips).
- [ ] Per-module cookies are shared between two `HTTP` objects of the same module.
- [ ] Doc comments cite `LuaHTTPSend.pas` / `httpsendthread.pas` lines.

## FMD2 references
- `baseunits/lua/LuaHTTPSend.pas:21-151` (each method wrapper), `:153-168` (metatable: properties and sub-objects)
- `baseunits/httpsendthread.pas:592-768` (request semantics the wrapper relies on)
- `baseunits/WebsiteModules.pas:353-387` (`PrepareHTTP`, `CreateHTTP`: per-module UA, cookies, connection queue)
- `baseunits/WebsiteModules.pas:278-283` (`MergeHTTPCookiesFromSetting`)
- `docs/LUA-REFERENCE.md:109-160` (HTTP object as modules use it)
