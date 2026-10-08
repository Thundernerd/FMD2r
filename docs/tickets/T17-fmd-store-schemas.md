# T17: `fmd-store`: `app.db` and `lists.db` schemas, migrations, repositories
Deps: T01

## Goal
Create the persistence layer: a fresh, normalised SQLite schema (via `rusqlite`) for application state and one FTS-indexed master list for all sites, with migrations and typed repositories that the engine, server, jobs and importer use.

## Scope (in/out)
In:
- `fmd-store` crate with `rusqlite` (bundled SQLite with FTS5), WAL mode, foreign keys on, busy timeout, and a migration runner (`rusqlite_migration` or a minimal versioned runner) for each DB.
- **`app.db`** tables (design the columns; these are the minimum):
  - `tasks` (id, module_id, manga link, title, save_to, status enum: Stopped/Waiting/Preparing/Downloading/Converting/Compressing/Finished/Failed/Disabled, enabled, sort order, date_added, date_last_downloaded, current chapter index, error text) — replaces FMD2's `downloads` table.
  - `task_chapters` (task_id, idx, link, name, custom filename, status, page count, current page) — replaces newline-joined `chapterslinks`/`chaptersnames`/`customfilenames`/`chaptersstatus` columns.
  - `task_pages` (task_id, chapter idx, page idx, url, container url, filename, status) — replaces `pagelinks`/`pagecontainerlinks`/`filenames`.
  - `favorites` (module_id, link, title, status, current chapter, save_to, enabled, order, date_added, date_last_checked, date_last_updated, cover url).
  - `downloaded_chapters` (module_id, manga link, chapter link) — normalised from FMD2's one-row-per-manga text blob.
  - `module_settings` (module_id, options JSON, HTTP overrides JSON (UA, cookies, proxy), limits overrides, enabled, cookie jar blob).
  - `accounts` (module_id, enabled, username, password, cookies, status) with username/password/cookies **encrypted at rest** (store ciphertext; the key/crypto helper is an injected trait so T31 can choose the scheme; provide a simple implementation using a key file in the data dir).
  - `settings` (key TEXT PRIMARY KEY, value JSON).
  - `events` (id, ts, kind, severity, module_id, task_id, title, body JSON, read flag) — inbox and history.
  - `module_files` (path, sha, last_modified, size) — synced Lua files (T29).
- **`lists.db`**: single `masterlist(module_id, link, title, alttitles, authors, artists, genres, status, summary, numchapter, added_jdn)` with primary key `(module_id, link)` and an FTS5 external-content index over title/alttitles/authors/artists/genres/summary kept in sync with triggers. One table avoids FMD2's 125-ATTACH limit.
- Repositories with focused methods (no ORM): e.g. `TaskRepo::{create, get, list_by_status, update_status, set_chapters, set_pages, update_page, reorder, delete}`, `FavoriteRepo`, `DownloadedChaptersRepo::{mark, contains, list_for}`, `ModuleSettingsRepo` (implements T06's `ModuleSettingsStore` trait), `AccountRepo`, `SettingsRepo::{get<T>, set<T>}`, `EventRepo::{push, list, mark_read}`, `ModuleFileRepo`, `MasterListRepo::{replace_module(module_id, rows), upsert, search(query, filters, page) , count}`.
- Bulk import path for master list (prepared statement in one transaction; ≥ 100k rows/s target on a laptop).
- A connection-pool or `Mutex<Connection>` strategy suitable for calling from async (spawn_blocking) and from worker threads; document it.

Out: settings semantics (T18); FMD2 import (T32); 7z download (T26).

## Seams under test
Public `fmd-store` repository APIs against a temp-dir database:
- Opening a fresh DB runs all migrations; reopening is a no-op; schema version recorded.
- `TaskRepo`: create a task with 3 chapters and 10 pages, update one page's status, reload → state preserved; `list_by_status(Downloading)` filters.
- `DownloadedChaptersRepo::mark` then `contains` true; idempotent.
- `MasterListRepo::replace_module("m", rows)` then `search("one piece")` finds by title and alttitle (FTS), `search` with genre include/exclude filters, pagination stable.
- `AccountRepo` stores ciphertext (raw SQL read shows no plaintext) and returns plaintext through the API.
- Event push/list/mark_read.

## Acceptance criteria
- [ ] Both DBs migrate from empty; migrations are forward-only and tested.
- [ ] Every table listed exists with sensible indexes (status, module_id, order).
- [ ] FTS5 search works with prefix queries and stays in sync on insert/update/delete.
- [ ] Repositories return typed domain structs and `thiserror` errors; no `unwrap` outside tests.
- [ ] Bulk master-list import of 100k synthetic rows completes in a test under a generous time bound.

## FMD2 references
- `baseunits/DownloadsDB.pas:63-167` (FMD2 `downloads` schema at :67-92, newline-joined columns this replaces)
- `baseunits/FavoritesDB.pas:51-163` (`favorites` schema at :55-71)
- `baseunits/DownloadedChaptersDB.pas:36-171` (`downloadedchapters` schema at :124-129; `CleanStr` at :36)
- `baseunits/DBDataProcess.pas:143-153` (per-site list schema), `:328`, `:363` (table creation), `:1255` (`Search`), `:1367` (`Filter` incl. genres)
- `baseunits/uDownloadsManager.pas:19-31` (download statuses), `:1445-1499` (DB write points)
- `baseunits/WebsiteModulesSettings.pas:94-171` (per-module settings shape)
- `baseunits/WebsiteModules.pas:545-696` (`modules.json` load/save: settings, cookies, account fields)
