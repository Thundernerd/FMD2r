# T25: Library: favorites API, grid, new-chapter check job, auto-download
Deps: T20, T24

## Goal
The Library: favorites stored in `app.db`, shown as a cover grid with filter chips, and a background job that checks favorites for new chapters at startup and on an interval, then auto-downloads or posts inbox items, like FMD2's favorites manager.

## Scope (in/out)
In:
- Endpoints: `GET /api/favorites?filter=&q=`, `POST /api/favorites` (from series), `PATCH /api/favorites/{id}` (enabled, save_to, title), `DELETE /api/favorites/{id}`, `POST /api/favorites/check` (all or ids), `POST /api/favorites/{id}/check-missing`, `GET /api/jobs/favorites` (state/progress).
- `FavoritesChecker` job in `fmd-core`: runs at startup (setting) and every N minutes (setting); concurrency bounded (threads setting, per-module limits); for each enabled favorite runs `OnGetInfo`, diffs chapter links against `downloaded_chapters` (and FMD2's `currentchapter` semantics), updates status/title/last-checked/last-updated; for new chapters either creates/extends a download task (auto-download setting) or pushes an inbox item listing them; also detects completed series (status change) like FMD2. Emits `job.favorites.*` events.
- "Check missing chapters" variant (chapters present on site but not downloaded, not only new ones).
- Library page: cover grid (cover via `/api/covers`), badges for new chapters, filter chips (All, New, Ongoing, Completed, Disabled, per-website), search, sort; "Check now" button with job progress; tap → series page. Series page "Add to library" button wired.

Out: list updates (T26).

## Seams under test
- Public `fmd-core` `FavoritesChecker` API with fixture module whose stub HTTP returns 3 chapters, store has 2 marked downloaded: run check → 1 new chapter; with auto-download on → a task with that chapter exists; off → one inbox event.
- HTTP handlers via `oneshot`: add favorite → list contains it; check endpoint triggers the job (fake job trait asserts call).
- Frontend: Vitest on filter logic; Playwright smoke on mock API: grid renders, chip filters, check button shows progress.

## Acceptance criteria
- [ ] Diff semantics match `uFavoritesManager.pas` (new vs missing chapters, status update).
- [ ] Startup + interval scheduling configurable; no overlapping runs.
- [ ] Auto-download and inbox behaviours both tested.

## FMD2 references
- `baseunits/uFavoritesManager.pas:302-531` (`TFavoriteThread.Execute`, `DoCheck` :329, `DoCheckMissing` :397), `:607-777` (`TFavoriteTask`: scheduling, thread pool), `:832-928` (`CheckForNewChapter`, `CheckForMissingChapters`), `:954-1178` (`ShowResult`: auto-download vs notify, completed series), `:1213-1300` (add/replace/delete), `:1356` (`AddToDownloadedChaptersList`)
- `baseunits/FavoritesDB.pas:55-163` (stored fields)
- `mangadownloader/forms/frmMain.pas:1871-1966` (`tmCheckFavoritesTimer`: interval check), `:2042` (`tmStartupTimer`)
- `mangadownloader/forms/frmNewChapter.pas` (new-chapter prompt this replaces with the inbox)
