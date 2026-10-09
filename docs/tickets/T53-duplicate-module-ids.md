# T53: Modules that share an ID crash the Settings and Discover pages
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification (F3). Upstream `lua/modules/Manga1001.lua:18-19` registers two websites under one ID:
```lua
AddWebsiteModule('1d09f3bea8f148fa9e9215fc578fedcd', 'https://manga1001.win')
AddWebsiteModule('1d09f3bea8f148fa9e9215fc578fedcd', 'https://hachiraw.win')
```
`GET /api/modules` returns that ID twice (both named "HachiRaw"). The module picker keys its list by ID (`{#each matches as m (m.id)}`, `web/src/lib/components/settings/ModuleSettings.svelte:105`), Svelte throws `each_key_duplicate`, and `/settings` renders "500 Internal Error", so no setting can be changed in the UI. The Discover page's website picker keys the same way (`web/src/lib/components/discover/WebsitePicker.svelte:47`), and both entries are in the "Raw" group, so it can hit the same error.

FMD2 keeps both: `TLuaWebsiteModulesLoader` adds every module `Init` creates to the list without checking IDs (`baseunits/lua/LuaWebsiteModules.pas:523-589`), `LocateModuleByHost` finds each by its `RootURL` (`baseunits/WebsiteModules.pas:500-534`), and `LocateModule` by ID returns one of them (`baseunits/WebsiteModules.pas:470-498`). FMD2r's registry already does the same: `ModuleRegistry::from_modules` keeps both, `get` returns the first by ID (`crates/fmd-lua/src/module/loader.rs:126-141`). So the catalog stays as it is, and the API and UI must cope with a repeated ID.

## Scope (in/out)
In:
- `ModuleSummary` (`crates/fmd-server/src/module_settings.rs:23`) gains the module's `root_url`, so the two entries can be told apart. Keep both in `GET /api/modules`, as FMD2's website list does.
- Keys in every `{#each}` over modules that can't collide (e.g. `id` plus `root_url`, or the index): the Settings module picker, the Discover website picker, and any other list over `/api/modules`.
- Pickers show the root URL (or host) when two entries share a name.
- Selecting either entry edits that ID's settings, which both share (as FMD2's `modules.json`, which is keyed by ID: `baseunits/WebsiteModules.pas:545-700`).

Out: deduplicating IDs in the catalog (FMD2 doesn't). Which of two same-ID modules an ID lookup returns (`ModuleRegistry::get` is already FMD2-faithful).

## Seams under test
- `fmd-server`: `GET /api/modules` with a fixture module file that calls `NewWebsiteModule` twice with one ID and two root URLs returns two entries with distinct `root_url`s.
- Web component tests: the Settings module picker and the Discover website picker render a module list with a repeated ID without throwing, and show both entries.

## Acceptance criteria
- [ ] `/settings` and `/discover` load with the bundled upstream snapshot (Manga1001.lua included).
- [ ] Both HachiRaw entries are listed and distinguishable.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks and tests pass.

## FMD2 references
- `lua/modules/Manga1001.lua:18-19`
- `baseunits/lua/LuaWebsiteModules.pas:523-589` (adding a file's modules)
- `baseunits/WebsiteModules.pas:470-534` (`LocateModule`, `LocateModuleByHost`)
- `baseunits/WebsiteModules.pas:545-700` (`modules.json`, keyed by ID)
