# T74: Named download destinations
Deps: none

## Goal
There is one download folder, `saveto.default_dir` ("Download folder", `web/src/lib/settings/sections.ts:114`; `crates/fmd-core/src/settings/model.rs:187-189`). A task saves there unless its "Save to" box on the series page was edited by hand (`web/src/lib/components/series/DownloadBox.svelte:93-96`, filled from `default_dir` at `web/src/routes/series/+page.svelte:56`; resolved in `save_to`, `crates/fmd-core/src/download/manager.rs:699-711`). Keeping, say, manga and manhwa, or two disks, apart means typing a path each time. Library series keep the folder they were added with (`favorite_save_to`, `crates/fmd-core/src/favorites.rs:68-85`), and the UI cannot change it, though `PATCH /api/favorites/{id}` accepts `save_to`. FMD2 also lets each website override the folder (`OverrideSettings.SaveToPath`, `baseunits/WebsiteModulesSettings.pas:50`, :55, applied by `OverrideSaveTo`, `mangadownloader/forms/frmMain.pas:5631-5643`), which FMD2r doesn't have.

Let the user define several named destinations and pick one wherever a download folder is chosen.

## Scope (in/out)
In:
- **Setting:** `saveto.destinations`, a list of `{ name, path }`, one of them the default. A migration turns today's `default_dir` into the first destination, named "Downloads", as the default. Keep `default_dir` working for existing API clients and the FMD2 import (or replace it, with the migration and `crates/fmd-import/src/settings.rs` updated). Names are unique and not empty; paths are not empty.
- **Settings → Save to:** add, rename, edit, remove and reorder destinations, and choose the default. The default can't be removed. A path that doesn't exist or isn't writable gets a warning (not an error, as a disk may be unmounted for a while).
- **Per website:** a module's settings (Settings → Website modules) can set its default destination, as FMD2's `SaveToPath` override. Order: what the user picks > the website's destination > the default destination.
- **Series page:** "Save to" becomes a destination picker plus a "Custom folder…" choice that shows today's free-text box. It starts on the website's or the default destination. The full resolved folder (with the manga folder) is shown under it.
- **Library:** adding a series to the library uses the destination chosen on the series page. A library series' destination can be changed from the library, through the existing `PATCH /api/favorites/{id}` `save_to`. Files already downloaded are not moved; say so in the UI.
- **FMD2 import:** a module's `OverrideSettings.SaveToPath`, today reported as unmapped (`crates/fmd-import/src/modules.rs:135-137`), becomes that website's destination (a destination is created for each distinct path, through the import's path maps).
- **Tasks** store the resolved path, as today, so renaming or removing a destination doesn't affect queued or finished tasks.
- **Docker:** document mounting more than one host folder (e.g. `/data/manga`, `/data/manhwa`) in `compose.yaml` comments and the README. Today only `MANGA_DIR` → `/data/downloads` is mounted (`compose.yaml:31-32`).

Out: downloading the same chapters to several destinations at once; moving existing files when a destination changes (FMD2 asks to move them, `RS_DlgMoveSaveToFiles`, `frmMain.pas:963-964`; a possible follow-up); per-destination output formats.

## Seams under test
- `fmd-core`: the settings migration turns `default_dir` into the default destination; the save folder resolves to user pick > website destination > default; validation rejects duplicate or empty names and removing the default.
- `fmd-server`: `POST /api/tasks` with a destination's path saves there; changing a library series' `save_to` makes its next download go to the new folder.
- `fmd-import`: an FMD2 `saveto/SaveTo` becomes the default destination, and a module's `SaveToPath` override becomes its website destination.
- Web component tests: the series page's picker starts on the website's destination when it has one, and shows the free-text box for "Custom folder…"; the Settings section adds, renames and removes destinations and can't remove the default.
- Playwright: add a second destination in Settings, download a chapter to it from the series page, and see the task's folder in the queue.

## Acceptance criteria
- [ ] Several named destinations can be set up, with one default and optional per-website defaults.
- [ ] The series page and the library let the user pick a destination; existing installs keep their current folder as the default.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
- `baseunits/WebsiteModulesSettings.pas:50`, :55: the per-module `SaveToPath` override.
- `mangadownloader/forms/frmMain.pas:5631-5643` (`OverrideSaveTo`), :2685-2710 (resolving the task folder), :963-964 (offering to move files when a favorite's folder changes).
