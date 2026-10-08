# T26: Discover: list update job, FMD2-DB import, FTS search, facets, tri-state genres
Deps: T14, T17, T22

## Goal
Browse and search each site's full manga list: build lists by running the module's update-list callbacks, or bootstrap from FMD2-DB prebuilt `<site>.7z` dumps, store everything in `lists.db`, and search it with FTS, facets and tri-state genre filters on the Discover page.

## Scope (in/out)
In:
- `ListUpdater` job in `fmd-core`, per module: `OnBeforeUpdateList` → `OnGetDirectoryPageNumber` (per directory if `TotalDirectory` > 1, `CurrentDirectoryIndex` set) → `OnGetNameAndLink` for each page index (parallel within limits; stop early when the module sets `SortedList` and a page yields only already-known links) → optional `OnGetInfo` per new title (setting; FMD2 "update list with info") → `OnAfterUpdateList`; `UPDATELIST.UpdateStatusText` surfaces as job progress text. Writes via `MasterListRepo` (merge, not replace, when stopping early). Emits `job.lists.*` events.
- FMD2-DB import: download `https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/<website>.7z` (URL template from settings), extract with `sevenz-rust`, read the contained SQLite file (FMD2 per-site schema), bulk-import into `masterlist` for that module.
- Endpoints: `GET /api/modules` (id, name, category, list size, last updated, capabilities), `POST /api/lists/{module}/update`, `POST /api/lists/{module}/import-db`, `GET /api/lists/search?module=&q=&genres_include=&genres_exclude=&status=&page=` (FTS + filters), `GET /api/lists/facets?module=&q=` (genre/status counts).
- Discover page: website picker (search, category groups), search box with debounce, facet sidebar/drawer, tri-state genre chips (include/exclude/ignore), status filter, results list/grid with pagination/infinite scroll, "update list"/"download DB" actions with job progress, tap → series page.

Out: favorites (T25).

## Seams under test
- Public `fmd-core` `ListUpdater` with a fixture module (3 directory pages, `SortedList` true) and stub HTTP: full update stores N rows; second run stops after the first page whose links are all known.
- FMD2-DB import from a small fixture `.7z` containing an FMD2-schema SQLite file → rows in `masterlist` with mapped columns (`jdn` → `added_jdn`).
- HTTP handlers via `oneshot`: search with include `Action` exclude `Romance` returns the expected fixture rows; facets counts correct.
- Frontend: Vitest on tri-state chip state → query params; Playwright smoke on mock API.

## Acceptance criteria
- [ ] Update flow and early stop match `uUpdateThread.pas`.
- [ ] 7z import handles FMD2-DB files (test with one real small site DB if licence allows, else synthetic).
- [ ] Search over 100k rows returns in < 100 ms on a laptop (bench, not CI-gated).

## FMD2 references
- `baseunits/uUpdateThread.pas:173-313` (`TUpdateListThread.Execute`: directory pages, name/link, info), `:626-780` (`TUpdateListManagerThread.Execute`: orchestration, SortedList early stop), `:326-352` (`ExtractFile`: 7z DB), `:842-903` (`GetNext`)
- `baseunits/lua/LuaWebsiteModules.pas:154-244` (update-list callbacks), `baseunits/lua/LuaUpdateListManager.pas:18-42`
- `baseunits/DBDataProcess.pas:143-153` (FMD2-DB schema), `:1255` (`Search`), `:1367` (`Filter`: genres include/exclude, status)
- `baseunits/DBUpdater.pas` (FMD2-DB download/extract), `dist/config.json` (`db_url` template)
- `docs/LUA-REFERENCE.md:348-448` (update-list callbacks)
