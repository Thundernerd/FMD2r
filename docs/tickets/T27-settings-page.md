# T27: Settings page, including per-module options and limits
Deps: T06, T18, T22

## Goal
A single Settings page with a table of contents that edits the T18 settings model, plus a per-module section to edit each module's declared options (`AddOption*`), limits and HTTP overrides.

## Scope (in/out)
In:
- Endpoints: `GET/PATCH /api/settings` (if not already exposed by T21), `GET /api/modules/{id}/settings` (declared options with kind/caption/default/items from `ModuleDef`, current values, limits with module defaults, HTTP overrides), `PATCH /api/modules/{id}/settings`.
- Settings page: sticky table of contents (sections mirror T18 groups), form controls generated from schema/typed definitions, inline validation errors from the API, save bar with dirty state, rename-template preview (calls a small `POST /api/preview-rename` using T19's `custom_rename` if merged, else client-side approximation flagged as such).
- Per-module section: module picker (search), checkbox/edit/spin/combo controls for `AddOption*` options, limits (max tasks, threads per task, connections; "use module default"), user agent, cookies, proxy, enabled toggle.

Out: accounts/login UI (T31).

## Seams under test
- HTTP handlers via `oneshot` with a fixture module declaring one option of each kind: `GET` returns all four with defaults; `PATCH` with a combo index out of range → 422; valid patch persists and `MODULE.GetOption` (via T06 store trait) sees the new value.
- Frontend: Vitest on form-generation from option definitions and dirty tracking; Playwright smoke on mock API: change an option, save, reload shows it.

## Acceptance criteria
- [ ] Every T18 setting editable; validation errors shown inline.
- [ ] Every option kind from `AddOption*` rendered correctly.
- [ ] Mobile layout: TOC collapses to a dropdown.

## FMD2 references
- `baseunits/lua/LuaWebsiteModules.pas:757-818` (`AddOption*` kinds, defaults, combo items), `:921-950` (`GetOption`)
- `baseunits/WebsiteModules.pas:329-352`, `:422-459` (option binding and types)
- `baseunits/WebsiteModulesSettings.pas:94-171` (per-module settings)
- `mangadownloader/forms/frmMain.lfm` (FMD2 options tab layout and groupings), `mangadownloader/forms/frmMain.pas:5803-6168`
