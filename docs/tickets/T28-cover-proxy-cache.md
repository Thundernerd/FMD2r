# T28: Cover proxy and disk cache
Deps: T10, T21

## Goal
Serve manga covers to the browser through `/api/covers/...`, fetched with the owning module's cookies, user agent and referer (sites often block hotlinking), and cached on disk.

## Scope (in/out)
In:
- `GET /api/covers?module={id}&url={cover_url}` (or an opaque signed key route `/api/covers/{key}`), fetching via a `fmd-http` session for that module (cookie jar, UA override, `Referer: <module RootURL>/`), respecting the module connection queue.
- Disk cache under the data dir keyed by hash(module, url): stores body + content type + ETag/Last-Modified; serves with `Cache-Control` and `ETag`; revalidates after TTL (setting); size cap with LRU eviction.
- Request coalescing: concurrent requests for the same cover make one upstream fetch.
- Optional resize to thumbnail widths (`?w=`) with the `image` crate, cached separately.
- Only http(s) URLs; reject private-network targets unless they match the module's host (SSRF guard).
- Helper for other endpoints to rewrite cover URLs into proxy URLs.

Out: anti-bot handling (T30 makes covers behind Cloudflare work automatically through the shared HTTP layer).

## Seams under test
HTTP handler via `oneshot` with a stub upstream server:
- First request fetches upstream with the module's UA, cookie and Referer (assert on the stub); second request served from disk (stub hit count stays 1).
- `If-None-Match` from browser → 304.
- 10 concurrent requests → 1 upstream fetch.
- URL to `http://127.0.0.1` not matching module host → 400.
- `?w=200` returns a 200-px-wide image.

## Acceptance criteria
- [ ] Module cookies/UA/referer applied.
- [ ] Cache persistence across restarts, size cap enforced.
- [ ] SSRF guard tested.

## FMD2 references
- `baseunits/uGetMangaInfosThread.pas:168-191` (`LoadCover`: how FMD2 fetches covers with the module's HTTP settings)
- `baseunits/WebsiteModules.pas:353-387` (`PrepareHTTP`/`CreateHTTP`: per-module UA/cookies)
- `baseunits/FileCache.pas` (FMD2's file cache)
