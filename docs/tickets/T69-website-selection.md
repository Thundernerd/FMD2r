# T69: Choose which websites Discover lists
Deps: none

## Goal
Discover's website picker (`web/src/lib/components/discover/WebsitePicker.svelte`) lists every loaded module, a few hundred, grouped by category. Finding the sites you actually use means searching or scrolling a long way. FMD2 has an Options → "Website selection" page (`tsWebsiteSelection`, a checkbox tree by category with a search box, `edWebsitesSearch` / `vtOptionMangaSiteSelection`, `mangadownloader/forms/frmMain.pas:3315-3317`). Only the checked websites appear in its website dropdown (`cbSelectManga`), and "all websites" searches cover only those (`SitesList`, `baseunits/DBDataProcess.pas:649-683`, :1459). The choice is stored as `general/MangaListSelect`, a comma-separated list of module IDs (`frmMain.pas:5990-6004`, read at :6464; modules no longer loaded are dropped).

## Scope (in/out)
In:
- **Setting:** `general.selected_websites`, a list of module IDs (`crates/fmd-core/src/settings/model.rs`), exposed through `GET/PATCH /api/settings`. IDs of modules that aren't loaded are kept but ignored, so a module that comes back later is still selected.
- **Default:** a fresh install selects nothing. On the first start after this upgrade, existing installs select every module that already has a list in `lists.db`, so nobody's Discover page goes empty.
- **Managing it:** a "Websites" section in Settings: modules grouped by category with a checkbox each, a search box, a count of the selected modules, and "Select all" / "Select none" for the modules currently shown. It saves with the rest of the page's settings.
- **Discover:** the website picker lists only the selected websites. "All websites" searches and browses only the selected ones. A "Manage websites" link next to the picker opens the Settings section. With nothing selected, Discover shows an empty state that links there instead of an empty list.
- **FMD2 import:** `crates/fmd-import/src/settings.rs` maps `general/MangaListSelect` to `selected_websites`.

Out: the selection doesn't limit anything else. "Add by URL", the library, favorites checks, the series page, and Settings → Website modules keep working with every loaded module (as in FMD2, where the selection only drives the manga list). Scheduled list updates are also out of scope.

## Seams under test
- `fmd-core`: the setting round-trips, and unknown module IDs survive a save. The upgrade migration selects the modules with a list in `lists.db`; a fresh install selects none.
- `fmd-server`: the list search with no module filter returns only titles from selected websites.
- `fmd-import`: `MangaListSelect = "a,b"` imports as `["a", "b"]`.
- Web component tests: `WebsitePicker` shows only the selected modules; the Settings section's search, checkboxes and "Select all" on a filtered view update the draft; Discover with no selection shows the empty state and the link.
- Playwright: select two websites in Settings, then Discover's picker lists exactly those two.

## Acceptance criteria
- [ ] Discover lists and searches only the websites the user selected.
- [ ] The selection can be managed from Settings, is imported from FMD2, and existing installs keep the websites they already have lists for.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
- `mangadownloader/forms/frmMain.pas`: `tsWebsiteSelection`, `vtOptionMangaSiteSelection`, saving `MangaListSelect` (:5990-6004), loading it (:6464-6487).
- `baseunits/DBDataProcess.pas`: "all sites" search limited to `SitesList` (:649-683, :1459).
- `baseunits/FMDOptions.pas:94`, :250: the default selection comes from `config.json`'s `default_selected_websites`.
