# T77: Fix the Settings → Websites layout (hidden search box, ragged long names)
Deps: none

## Goal
Two layout bugs in Settings → Websites (`web/src/lib/components/settings/WebsiteSelection.svelte`):
- **The search box and "Select all" / "Select none" are hidden.** Their row uses `class="bar"` (:61), but `.bar` is the global progress-bar class (`web/src/lib/styles/app.css:109-114`: `height: 6px; overflow: hidden`). The component's scoped `.bar` rules (:106-115) don't override the height or overflow, so the row is cut to a 6px sliver. The other `class="bar"` uses are real progress bars (`QueueDock.svelte:20`, `JobsPanel.svelte:91`, `MangaBakaPanel.svelte:87`, `TaskRow.svelte:92`, `ListActions.svelte:111`, `routes/+page.svelte:119`).
- **Long website names knock the list out of line.** Each category is a wrapping flex row of checkboxes (`.group`, :116-123) whose items have `min-width: 200px` (`.choice`, :128-133) and grow with their label, so a long name makes its item wider than the rest and the rows no longer line up.

## Scope (in/out)
In:
- Give the toolbar row its own class name (e.g. `toolbar`) so it no longer picks up the progress-bar styles. The search box, "Select all" and "Select none" show at full size and wrap on narrow screens.
- Lay out each category's websites in even columns: every item the same width, with a long name truncated with an ellipsis and its full name (and host, for repeated names) in a `title` tooltip. The checkbox and its label stay one click target. On a phone it is a single column.

Out: what the section does (selection, search behaviour, counts); renaming the global `.bar` class.

## Seams under test
- Web component test (`WebsiteSelection.test.ts`): the search box and both buttons are rendered and usable (typing filters the list; "Select all" selects the shown websites). A website with a long name has its full name in the `title`.
- Playwright: on `/settings#section-websites` the search box's height is at least that of a normal input (not 6px), and the checkboxes of one category share the same left positions in each column.

## Acceptance criteria
- [ ] The Websites section's search box and buttons are fully visible.
- [ ] Long website names no longer misalign the list.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None.
