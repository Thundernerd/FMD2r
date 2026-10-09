# T76: Picking a website module doesn't jump the page
Deps: none

## Goal
In Settings → Website modules, clicking a module in the list feels like a full page reload. `selectModule` (`web/src/routes/settings/+page.svelte:124-132`) navigates to `?module=<id>` with `goto(url, { replace: true, reset: false })`. Without `noScroll`, SvelteKit scrolls the window to the top, so the list and the panel jump out of view. The effect that loads the module (:107-122) then clears `moduleView` and `moduleDraft` at once, so the panel switches to "Loading…" (`web/src/lib/components/settings/ModuleSettings.svelte:225`) and back, changing the page height twice.

## Scope (in/out)
In:
- Selecting a module keeps the window's scroll position and the list's own scroll position. The URL still updates (`?module=<id>`, replacing the history entry), so a link or reload opens that module.
- While the next module's settings load, the panel keeps showing the previous module (dimmed or with a small busy indicator) instead of collapsing to "Loading…". "Loading…" stays for the first load, when there's nothing to show yet.
- A slow response for an earlier click can't overwrite a later one (the existing check against `page.url.searchParams` at :116 stays).
- The unsaved-changes prompt on switching (:126-128) keeps working.

Out: the list's order and grouping (T72); the panel's contents.

## Seams under test
- Web component test (`ModuleSettings` or the route): while the second module's settings are pending, the first module's panel stays rendered and marked busy; once they resolve, the second module's panel shows.
- Playwright: scroll the page down, pick a module lower in the list; `window.scrollY` is unchanged afterwards and the module's panel shows its name.

## Acceptance criteria
- [ ] Picking a module updates the panel in place, with no jump to the top and no flash of "Loading…".
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None.
