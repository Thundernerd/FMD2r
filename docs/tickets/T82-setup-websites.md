# T82: Setup step: websites
Deps: T78

## Goal
Discover lists only the websites the user selected (T69, `general.selected_websites`), and a fresh install selects none (`crates/fmd-core/src/settings/model.rs:62`), so a new user's Discover page starts empty. Settings → Websites edits the selection with `WebsiteSelection` (`web/src/lib/components/settings/WebsiteSelection.svelte`). A new user should choose their websites during setup (T78).

## Scope (in/out)
In:
- A "Websites" step that reuses `WebsiteSelection` (the same component, with its search, categories and "Select all"/"Select none"), with a short explanation: these are the websites Discover lists and searches, the library and "Add by URL" work with every website, and it can be changed later in Settings → Websites.
- Next needs at least one website selected, and saves `general.selected_websites`. The step says why when none is selected.
- After this step, the finish step (T78) mentions that each website's list still has to be fetched from Discover ("Update list" or "Get the list from FMD2-DB"), with a link there.

Out: fetching lists from inside the wizard; changing `WebsiteSelection` beyond what fitting into the wizard needs.

## Seams under test
- Web component test: Next is disabled with nothing selected; selecting two websites and pressing Next saves both IDs.
- Playwright (mocked API): select two websites during setup, finish, and Discover's website picker lists exactly those two.

## Acceptance criteria
- [ ] Setup makes the user choose which websites Discover lists.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
- FMD2's equivalent is Options → "Website selection" (`tsWebsiteSelection`, `mangadownloader/forms/frmMain.pas:234`).
