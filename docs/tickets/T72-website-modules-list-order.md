# T72: A clear, sorted list in Settings → Website modules
Deps: none

## Goal
The "Website modules" section (`web/src/lib/components/settings/ModuleSettings.svelte`) shows a search box and an unlabelled scroll box (`<ul class="list">`, :101-130, max-height 360px) of buttons, with the selected module's settings beside it. It doesn't read as a list to pick from. Its order looks random: the modules come from `GET /api/modules` sorted by module ID (`crates/fmd-server/src/lua_catalog.rs:62`, :71), and IDs are hashes such as `ba5c1a22af434aaca6c8c6874b7f54ec`. Discover's picker, by contrast, groups by category and sorts by name (`web/src/lib/components/discover/WebsitePicker.svelte:21-36`).

## Scope (in/out)
In:
- **Sorted:** modules are listed by name, case-insensitively. Modules sharing a name are ordered by host, which is already shown next to them (`repeatedNames` / `moduleHost`, `web/src/lib/modules.ts`).
- **Grouped by category,** with a small heading per category, the same grouping and order as the Discover picker. Share the grouping and sorting helper between the two components instead of copying it.
- **Recognisable as a picker:** a visible label ("Pick a website to edit its settings"), and a count ("312 modules", or "12 of 312" while searching). The selected module stays highlighted and is scrolled into view when the page opens with `?module=`.
- **Search** also matches the category and the host, as Discover's search matches the category.
- **Customised modules are marked:** a module whose limits, HTTP settings or options differ from its defaults gets a small marker, so you can find what you changed. `ModuleSummary` gains a field for this (e.g. `customized: bool`). Update `openapi.json` and the web types.
- Keyboard: arrow keys move through the list, and Enter selects.

Out: choosing which websites Discover lists (T69); the layout of the module's settings panel itself.

## Seams under test
- Web component tests (`ModuleSettings.test.ts`): modules given in ID order render in name order under their category headings; two modules with the same name are ordered by host; the count reflects the search; searching a category name finds its modules; a customised module shows the marker.
- `fmd-server`: `GET /api/modules` reports `customized: true` after a module's settings are changed through `PATCH /api/modules/{id}/settings`, and `false` after they are reset.
- Playwright: `/settings?module=<id>` opens with that module selected and visible in the list.

## Acceptance criteria
- [ ] The Website modules list is labelled, alphabetical, grouped by category, and marks modules with custom settings.
- [ ] The web checks, unit tests and e2e tests pass, and fmt, clippy `-D warnings` and `cargo test --workspace` pass.

## FMD2 references
`TWebsiteModuleSettings` (`baseunits/WebsiteModulesSettings.pas:60`) holds the per-module settings this section edits.
