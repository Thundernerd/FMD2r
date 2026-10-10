# T79: Setup step: download folders
Deps: T78

## Goal
T74 added named download destinations: several folders, one of them the default (`saveto.destinations`), edited in Settings → Save to with `DestinationsEditor` (`web/src/lib/components/settings/DestinationsEditor.svelte`). A new user should set them up during setup (T78), including adding more than one.

## Scope (in/out)
In:
- A "Download folders" step that reuses `DestinationsEditor` (the same component, not a copy), so the wizard and Settings stay alike. It starts with the default destination ("Downloads"), and the user can rename it, change its path, add more and choose the default.
- A short explanation above it: what a destination is, that each website can have its own (Settings → Website modules), and that it can be changed later.
- The folder check the editor already does (whether downloads can be saved in a folder) shows here too. A folder that can't be written to is a warning, not a blocker, as in Settings.
- Next is allowed when the destinations are valid (names unique and not empty, a default chosen), and saves them.
- When running in Docker, a hint that each folder must be mounted into the container (the README's Docker section).

Out: changing the editor itself; per-website destinations in the wizard.

## Seams under test
- Web component test: the step shows the default destination; adding a second one and pressing Next saves both through the settings API; a duplicate name keeps Next disabled.
- Playwright (mocked API): on a fresh install, add a destination in the step, finish setup, and see both in Settings → Save to.

## Acceptance criteria
- [ ] Setup lets the user create and name several download folders and choose the default.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None.
