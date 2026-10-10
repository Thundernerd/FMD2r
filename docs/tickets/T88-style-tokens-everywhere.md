# T88: Every font size and colour comes from a style token
Deps: none

## Goal
The web UI's colours, fonts and sizes are CSS custom properties in `web/src/lib/styles/tokens.css`, and almost every component uses them. A few don't: they hard-code a font size or a colour, so a different text size or theme (T89, T90, T91) would leave them unchanged. Move those values into tokens so that overriding the tokens restyles the whole app, with the layout staying the same.

## Scope (in/out)
In:
- **Font sizes in px** become tokens:
  - `.btn` 13px (`web/src/lib/styles/app.css:80`);
  - `TopNav.svelte:116` (20px);
  - `SeriesHeader.svelte:208`, `:244`, `:314` (34px, 13px, 26px);
  - `InboxPopover.svelte:122` (10.5px);
  - `ChapterList.svelte:224` (13px);
  - Library and Discover: the search box (15px; `web/src/routes/+page.svelte:299`, `web/src/routes/discover/+page.svelte:353`) and the card title (13.5px; `+page.svelte:391`, `discover/+page.svelte:373`).

  Use an existing `--fs-*` step where it's close enough, and otherwise add one (e.g. `--fs-title`, `--fs-h2`, `--fs-hero`). Two places showing the same thing get the same token. Sizes may move by up to 0.5px where that merges a near-duplicate.
- **Colours** become tokens:
  - the white text and its shadow on covers (`SeriesHeader.svelte:188-191`, `CoverThumb.svelte:91-94`, `web/src/routes/+page.svelte:371-374`) become `--on-cover` and `--on-cover-shadow`;
  - the series header's placeholder gradient (`SeriesHeader.svelte:190`) becomes `--cover-placeholder`;
  - the dialog and drawer scrims (`ImportDialog.svelte:215`, `FolderDialog.svelte:141`, `discover/+page.svelte:422`) become `--scrim`.

  Each token gets a value in the dark theme too, even if it's the same.
- **Dark theme tokens defined once.** `tokens.css` lists the dark values twice (:57-79 for the OS setting and :81-101 for `data-theme='dark'`). Keep both selectors but define the values once, e.g. with a shared selector list, so later themes don't have to copy them.
- **A check that keeps it this way:** a unit test or lint step that fails when a `.svelte` or `.css` file under `web/src`, other than `tokens.css`, has a `font-size` in px or a hex/`rgb()`/`rgba()` colour.

Out:
- Spacing, radii and widths: they're layout and stay as they are.
- Any visible change beyond the 0.5px merges.

## Seams under test
- The check above, run with the web unit tests.
- The existing unit and e2e tests still pass. Compare screenshots before and after of Library, Discover, a series page and Settings, in light and dark; they show no visible difference.

## Acceptance criteria
- [ ] No component hard-codes a font size or a colour; all of them use tokens from `tokens.css`.
- [ ] Dark theme values are written once.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None.
