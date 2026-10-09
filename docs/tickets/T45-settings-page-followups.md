# T45: Settings page follow-ups
Deps: none

## Goal
Finish the review follow-ups T27 left open (PR #46).

## Scope (in/out)
In:
- Report every invalid field in one `422`, not only the first (`Problem` gains a `fields` list; keep `field` for compatibility), and show all inline errors at once.
- Save global and module settings atomically: one endpoint or a transaction so a failure in one doesn't leave the other saved.
- Rename preview applies `remove_manga_name_from_chapter`, and reflects whether a chapter folder is generated and the real page extension.

Out: new settings.

## Seams under test
HTTP handlers via `oneshot` (multi-field 422; atomic save rolls back both on failure; preview output for the setting on/off). Vitest for showing several inline errors; Playwright (mock) for the save flow.

## Acceptance criteria
- [ ] A patch with three invalid fields shows three inline errors.
- [ ] A failing module patch leaves the global settings unchanged.
- [ ] Preview matches what the download engine writes for the same inputs.

## FMD2 references
- `baseunits/uBaseUnit.pas:1798` (`CustomRename`)
- `mangadownloader/forms/frmMain.pas:5803-5980` (`LoadOptions`)
