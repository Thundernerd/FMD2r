# T32: FMD2 importer
Deps: T11, T17

## Goal
One-time import of an existing FMD2 installation's userdata into FMD2r: download queue, favorites, downloaded chapters, per-module settings/accounts (`modules.json`) and global settings, so users can switch without losing state.

## Scope (in/out)
In:
- `fmd-import` crate: `import(fmd2_userdata_dir, store, opts) -> ImportReport` (counts, skipped rows with reasons), idempotent (re-running doesn't duplicate), dry-run mode.
- Sources (schemas documented from the Pascal):
  - `downloads.db` (`downloads` table): split newline-joined `chapterslinks`/`chaptersnames`/`pagelinks`/`pagecontainerlinks`/`filenames`/`customfilenames`/`chaptersstatus` into `tasks`/`task_chapters`/`task_pages`; map `taskstatus` integers to FMD2r statuses (in-progress ones → Stopped or resumable per option); keep order, dates, save-to.
  - `favorites.db` (`favorites` table): including `downloadedchapterlist` and `currentchapter`.
  - `downloadedchapters.db` (`downloadedchapters`: `id` = moduleid+link key, `chapters` newline-joined) → `downloaded_chapters` rows (reproduce `CleanStr`/id format).
  - `modules.json`: per-module settings (enabled, HTTP UA/cookies/proxy, limits), options values, cookies, and accounts — account username/password/cookies decrypted with FMD2's `DecryptString` (T11) then re-encrypted with FMD2r's at-rest scheme.
  - `settings.json`: map known keys to the T18 model (save-to dir, rename templates, output format, connections, favorites/update intervals); report unmapped keys.
- Path translation option for Windows paths (`C:\Manga\…` → a configured Linux root).
- CLI: `fmd2r import --from DIR [--dry-run] [--map-path 'C:\Manga=/data/manga']`; optional `POST /api/import` (upload a zip of userdata) if T21 merged.

Out: copying downloaded files.

## Seams under test
Public `fmd-import` API with fixture FMD2 userdata (small SQLite files built in test setup with FMD2's exact `CREATE TABLE` statements, plus a `modules.json`/`settings.json` sample including an account encrypted with FMD2's `EncryptString` vector from T11):
- Import → task count, chapter/page splits, statuses mapped; favorite with downloaded list; downloaded chapters queryable via `DownloadedChaptersRepo::contains`.
- Account password decrypted then readable via `AccountRepo` (plaintext equal), and DB holds FMD2r ciphertext.
- Second import → no duplicates; report shows skipped-as-existing.
- `--map-path` rewrites save-to paths.

## Acceptance criteria
- [ ] All five sources imported; unmapped data listed in the report.
- [ ] Idempotent and dry-run tested.
- [ ] Schema assumptions cite Pascal lines.

## FMD2 references
- `baseunits/FMDOptions.pas:288-295` (userdata file names)
- `baseunits/DownloadsDB.pas:63-167` (schema :67-92, write order), `baseunits/uDownloadsManager.pas:1445-1499` (how fields are joined), `:1638-1692` (`Restore`: how FMD2 reads them back), `:19-31` (status enum order = integer values)
- `baseunits/FavoritesDB.pas:51-163` (schema :55-71), `baseunits/uFavoritesManager.pas:1301-1355` (`Restore`/`Backup`)
- `baseunits/DownloadedChaptersDB.pas:36-171` (`CleanStr` :36, get/set :47-91, schema :124-129)
- `baseunits/WebsiteModules.pas:545-696` (`modules.json` load/save, accounts decrypted at :609-611)
- `baseunits/uBaseUnit.pas:1556-1590` (`EncryptKey`, `EncryptString`, `DecryptString`)
- `mangadownloader/forms/frmMain.pas:5803-5980` (`LoadOptions`: settings keys)
- `mangadownloader/forms/frmImportFavorites.pas` (FMD2's own favorites importer, for format edge cases)
