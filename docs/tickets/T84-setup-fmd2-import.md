# T84: Setup step: import from FMD2
Deps: T78

## Goal
People switching from FMD2 can import its userdata from the Library page ("Import from FMD2", `web/src/lib/components/library/ImportDialog.svelte`, opened from `web/src/routes/+page.svelte:152`). The import includes a dry run, a report and path maps for Windows paths. It brings the library and FMD2's settings, among them the download folder (`saveto/SaveTo` → `saveto.default_dir`, `crates/fmd-import/src/settings.rs:136`), the output format (:140-142) and the website selection (`MangaListSelect`, :38, :56). Those are what the later setup steps ask about. The setup (T78) should offer the import first, so those steps start from the imported values.

## Scope (in/out)
In:
- An optional "Import from FMD2" step right after the welcome step: "Coming from FMD2? Import your library and settings", with "Import" and "Skip".
- "Import" reuses the import flow from the Library page (upload, dry run with its report, path maps, then import), embedded in the step rather than as a dialog. Extract the dialog's content into a component that both the dialog and the step use, so they stay alike.
- After an import, the later steps (download folders, download format, websites) start from the imported values, and say they came from FMD2. They are still shown, so the user can confirm or change them.
- A failed or cancelled import leaves the user on the step, with the report, able to retry or skip.
- Importing during setup doesn't mark setup as finished (T78 decides that only by its own setting).

Out: changing what the import reads or maps (`crates/fmd-import`); importing several times in one setup.

## Seams under test
- Web component tests: the step's "Skip" goes on without calling the import API; "Import" runs the dry run and shows its report before the real import; after an import, the format step preselects the imported format.
- Playwright (mocked API, with the stand-in userdata upload `web/e2e/library.test.ts:66-71` uses): import during setup, then the download folders and websites steps show the imported values; the Library page's import dialog still works.

## Acceptance criteria
- [ ] A user coming from FMD2 can import during setup, and the following steps start from what was imported.
- [ ] The Library page's import keeps working, through the same component.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None beyond what `crates/fmd-import` already cites.
