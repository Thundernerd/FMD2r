# T24: Series page and add-by-URL
Deps: T14, T21, T22

## Goal
Given a manga URL (pasted into add-by-URL or picked from Discover/Library), resolve the module, fetch info via `OnGetInfo`, and show the Series page: metadata, cover, chapter list with downloaded state, and a download box that creates a task.

## Scope (in/out)
In:
- Endpoints: `POST /api/resolve` `{url}` → `{module_id, link}` (host match as FMD2's `LocateModuleByHost`, link made relative to RootURL as FMD2 stores it); `GET /api/series?module=&link=` → `MangaInfo` (title, alt titles, authors, artists, genres, status, summary, cover URL routed through `/api/covers/...`, chapters with `downloaded` flag from `downloaded_chapters`, `in_library` flag). Short-lived cache of info results.
- Errors mapped: unknown host → 404 with message; module returns `information_not_found` → 404; `net_problem` → 502.
- Series page: header (cover, title, status chip, authors), summary (expandable), genre chips, chapter list (virtualised for 1000+ chapters; select range/all/new; downloaded markers; sort toggle), download box (chapter selection summary, save-to (defaults from settings), output format, "Download" → `POST /api/tasks` from T23 if available else a stub), "Add to library" button (wired in T25).
- Add-by-URL in the top chrome hits `/api/resolve` and navigates.

Out: favorites persistence (T25); cover proxy implementation (T28; until then cover URLs may be direct).

## Seams under test
- HTTP handlers via `oneshot` with a real `WorkerPool` on a fixture module and stub HTTP: `POST /api/resolve` for `https://example.com/manga/1` → module `t`; `GET /api/series` returns the fixture's title and chapters, `downloaded` true for a chapter pre-marked in the store.
- Frontend: Vitest on chapter-selection logic (ranges, "new only"); Playwright smoke on mock API: paste URL → series page renders → select 2 chapters → Download calls the API with them.

## Acceptance criteria
- [ ] URL → module resolution matches FMD2 host matching (www/no-www, scheme-insensitive, subdomains as FMD2 does).
- [ ] Series endpoint returns everything the page shows; chapters keep module order.
- [ ] Page usable on phone; 2000-chapter list scrolls smoothly.

## FMD2 references
- `baseunits/WebsiteModules.pas:470-534` (`LocateModule`, `LocateModuleByHost`)
- `baseunits/uData.pas:85-208` (`GetInfoFromURL`: calling `OnGetInfo`, normalising results), `:217` (`AddInfoToData`)
- `baseunits/uGetMangaInfosThread.pas:59-158` (info fetch flow, cover loading at :168)
- `baseunits/lua/LuaWebsiteModules.pas:245-266` (`DoGetInfo`), `baseunits/lua/LuaMangaInfo.pas:18-37`
- `baseunits/uDownloadsManager.pas:1747-1768` (`GetDownloadedChaptersState`)
