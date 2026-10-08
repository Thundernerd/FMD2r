# T09: `fmd-http` client with Synapse semantics
Deps: T01

## Goal
A blocking HTTP façade, used from Lua worker threads, that reproduces how FMD2's `THTTPSendThread` (Synapse-based) behaves on the wire: URL normalisation, retries, redirects, decompression, per-module cookie jar, proxy and per-module connection limits. T10 wraps it for Lua; T30 adds the anti-bot hook.

## Scope (in/out)
In:
- `fmd-http` crate: `HttpClient` (shared, holds a `reqwest` async client on a shared tokio runtime and drives it with `Handle::block_on` from worker threads) and a per-call-site `HttpSession` (the Rust counterpart of one `THTTPSendThread`): request headers, response headers, cookies, document (body bytes), result code/text, last URL, user agent, mime type, retry count, timeout, terminated flag.
- Behaviour (each cited to `httpsendthread.pas`):
  - URL without scheme gets `https://` prepended.
  - If the request header list starts with `HTTP/` (i.e. it still holds the previous response headers), reset before sending.
  - Retry on transport errors and on status > 500, up to `RetryCount` (`-1` = infinite until terminated).
  - Follow 301/302/303/307 manually, at most 5 times (`FMaxRedirect`), as GET, adding `Referer: <previous url>` if none set; keep cookies across hops.
  - After the request, `Headers` holds the **response** headers.
  - `get/post/head` return `true` iff the response body is non-empty (even on 404).
  - `post`: a `Content-Type` request header is moved into `MimeType`; `text/html` becomes `application/x-www-form-urlencoded; charset=UTF-8`.
  - `xhr`: adds `X-Requested-With: XMLHttpRequest` and resets stale headers like FMD2.
  - Decompression: gzip, deflate, br, zstd.
  - Cookie jar per module (shared by all sessions of that module), with `AddServerCookies(url, cookies)`, merge, remove by name, clear, and serialisation for persistence.
  - Proxy: HTTP and SOCKS (4/5), global default plus per-session override (`SetProxy(type, host, port, user, pass)`).
  - Per-module connection queue: at most `MaxConnectionLimit` concurrent requests per module; `0` = unlimited.
  - Cancellation: a terminate token aborts waits, retries and in-flight requests.
  - `Reset` / `ResetBasic` semantics.
- A pluggable `Transport` seam so tests (and T15's `--record`/replay) can substitute recorded responses.

Out: Lua binding (T10); anti-bot hook (T30); record/replay CLI (T15).

## Seams under test
Public `fmd-http` API against a local test server (`wiremock`/`httpmock` or axum on 127.0.0.1):
- `session.get("example.test/x")` hits `https://…` (assert via a transport stub or recorded URL).
- 404 with body `"nope"` → `get` returns `true`, `result_code == 404`.
- 200 with empty body → `false`.
- 502, 502, 200 with `retry_count = 3` → success after 3 attempts; 500 is **not** retried.
- Chain of 6 redirects → stops after 5; 303 after POST becomes GET; `Referer` set.
- `post` with `Content-Type: text/html` header → request sent with `application/x-www-form-urlencoded; charset=UTF-8`.
- Response headers replace request headers after the call.
- br/zstd/gzip bodies decoded.
- Connection queue: with limit 2, a third concurrent request waits (assert max concurrency at the server).

## Acceptance criteria
- [ ] Every quirk above has a test and a doc comment citing `httpsendthread.pas` lines.
- [ ] No blocking call is made on a tokio runtime thread (document the threading contract).
- [ ] Cookie jar is shared per module and serialisable.
- [ ] Termination interrupts an in-progress retry loop within one timeout tick.

## FMD2 references
- `baseunits/httpsendthread.pas:592-719` (`DefaultHTTPRequest`: `https://` prefix at :616, reset on `HTTP/` at :617, retry loop with `ResultCode > 500` at :624-627, redirect loop 301/302/303/307 with `FMaxRedirect` and Referer at :631-680, body/result at :680-718)
- `baseunits/httpsendthread.pas:720-768` (`HEAD`, `GET`, `POST` MimeType rules at :738-746, `XHR`)
- `baseunits/httpsendthread.pas:404-443` (`THTTPQueue`: per-module connection limit)
- `baseunits/httpsendthread.pas:460-495` (`NormalizeHeaders`, `ParseHTTPCookies`, `InternalHTTPRequest`)
- `baseunits/httpsendthread.pas:496-554` (constructor defaults: UA, timeout, retry count, max redirect)
- `baseunits/httpsendthread.pas:769-837` (`MergeCookies`, `AddServerCookies`, termination, `RemoveCookie`)
- `baseunits/httpsendthread.pas:838-918` (`SetProxy`/`GetProxy`), `:924-960` (`Reset`, `ResetBasic`, `ClearCookies`, `ClearCookiesStorage`)
- `baseunits/httpsendthread.pas:332-393` (global default proxy/timeout/retry)
- `baseunits/httpcookiemanager.pas` (cookie storage semantics)
- `baseunits/BrotliDec.pas`, `baseunits/ZstdDec.pas`, `baseunits/GZIPUtils.pas` (supported encodings)
