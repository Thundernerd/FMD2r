# T85: Call FMD2-DB lists "ready-made lists" in the UI
Deps: none

## Goal
Discover offers to "Get from FMD2-DB" when a website has no list yet. FMD2-DB is the name of the upstream project (`dazedcat19/FMD2-DB` on GitHub) that publishes a ready-made list for each website, saving FMD2r from building it by crawling the site. The name means nothing to a user. Use "ready-made list" everywhere a user reads it; the error messages already say "has no ready-made list for…".

## Scope (in/out)
In:
- **Discover** (`web/src/lib/components/discover/ListActions.svelte`):
  - the button "Get from FMD2-DB" (:128) becomes "Get ready-made list";
  - "No list yet. Get it from FMD2-DB, or build it from the website (slow)." (:100) becomes "No list yet. Get a ready-made one, or build it from the website (slow).";
  - the progress text "Getting the list from FMD2-DB" (:28) becomes "Getting the ready-made list";
  - the failure messages (:38-47), e.g. "There is no ready-made list for MangaDex yet. Use Update list to build it from the website.", "Could not reach the ready-made lists to get the list of MangaDex…".
- **The server's messages** for the same failures (`crates/fmd-core/src/lists/jobs.rs:131-149`), worded the same as the web's.
- **Settings → Manga lists:** the field "FMD2-DB URL" (`web/src/lib/settings/sections.ts:212`) becomes "Ready-made lists URL". Its help says once where they come from: "From the FMD2-DB project by default. `<website>` is replaced by the module ID." (The current help says "module name", but `db_url` replaces it with the module ID, `crates/fmd-core/src/settings/model.rs:455`.)
- **Notifications:** the mock inbox text (`web/src/lib/api/mock.ts:64`) and any real server notification that names FMD2-DB.
- **Descriptions users or API readers see:** utoipa doc comments that end up in `openapi.json` (e.g. `crates/fmd-server/src/lists.rs:288`, `crates/fmd-server/src/events.rs:145`). Regenerate `openapi.json` and the web types.
- Update the tests and mocks that assert the old strings (`web/src/lib/components/discover/ListActions.test.ts`, `web/e2e/discover.test.ts:60-73`, `web/src/lib/api/mock-lists.ts:41`, and the Rust tests asserting the messages).

Out:
- **The default URL**, which points at the upstream repo and keeps its name (`crates/fmd-core/src/settings/model.rs:431-434`).
- **Identifiers:** `import_db`, `ImportDb`, `DbImporter`, `POST /api/lists/{module}/import-db`, the `update_lists.db_url` setting key. Renaming these would break the API and stored settings for no user-visible gain.
- **Code comments** that refer to the upstream project, which may keep the name where they mean the project itself.
- **Earlier tickets and docs** under `docs/` (history).

## Seams under test
- `fmd-core`: a failed import's message for a missing list reads "There is no ready-made list for …" and doesn't contain "FMD2-DB".
- Web component test (`ListActions.test.ts`): the button reads "Get ready-made list", and no failure message contains "FMD2-DB".
- Playwright (`web/e2e/discover.test.ts`): a website without a list gets one through "Get ready-made list".
- A check that no user-facing string in `web/src` (outside tests and mocks of the upstream URL) or in the server's messages contains "FMD2-DB", e.g. a test or a grep in CI.

## Acceptance criteria
- [ ] No user-facing text says "FMD2-DB", except Settings' one mention of where the lists come from.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
FMD2 never shows the project's name either: its menu says "Download manga list from FMD server" (`mangadownloader/forms/frmMain.lfm:5841`, handled by `mnDownload1ClickClick`, `frmMain.pas:3791`).
