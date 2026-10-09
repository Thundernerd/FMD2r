# T58: Readable inbox item bodies
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification (O2). The inbox shows raw JSON, e.g. `{"file":"modules/OrckuMangas.lua","names":["MANGAINFO.Artist"]}`, and a Lua stack trace with literal `\n\t`. The module updater stores structured bodies (`{file, names}` at `crates/fmd-core/src/module_updater.rs:648`, `{file, error}` at :667), `InboxItem` turns any non-string body into JSON text (`crates/fmd-server/src/inbox.rs:44-48`), and the popover prints it as-is (`web/src/lib/components/InboxPopover.svelte:80`).

## Scope (in/out)
In:
- The inbox API gives each item readable text: for the updater's kinds, e.g. "Unknown Host API names: MANGAINFO.Artist" and the error message with real line breaks. Keep the structured body available if the UI or other clients use it.
- The popover keeps line breaks (a stack trace stays readable) and doesn't overflow on long lines.

Out: new inbox item kinds. The favorites check's items (already text).

## Seams under test
- `fmd-server`: `GET /api/inbox` for a stored `{file, names}` event and a `{file, error}` event returns plain text bodies with no JSON syntax and real newlines.
- Web component test: the popover renders a multi-line body on separate lines.

## Acceptance criteria
- [ ] No inbox item shows JSON or escaped `\n`/`\t`.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks and tests pass.

## FMD2 references
None (FMD2 has no inbox; the updater reports in its own window, `mangadownloader/forms/frmLuaModulesUpdater.pas`).
