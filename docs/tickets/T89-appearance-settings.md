# T89: Appearance settings: light or dark, text size, accent colour
Deps: T88

## Goal
The UI follows the OS for light or dark (`prefers-color-scheme` in `web/src/lib/styles/tokens.css`), and its text size and colours are fixed. Add a Settings → Appearance section with the common changes: light, dark or system; a text size; an accent colour. The layout stays the same.

## Scope (in/out)
In:
- **A new `appearance` settings group** (`crates/fmd-core/src/settings/model.rs`, next to `general`), stored on the server like the rest, so every device gets the same look:
  - `mode`: `system` (default), `light` or `dark`;
  - `text_size`: `small`, `normal` (default), `large` or `larger`;
  - `accent`: one of a fixed set of swatches (`teal` as default, the current colour, plus e.g. blue, green, purple, orange, red). Each swatch has a light and a dark value for `--accent`, `--accent-fg` and `--accent-soft`, chosen so text on the accent and links stay at least 4.5:1 contrast. No free colour picker; that's what `custom.css` (T91) is for.

  Add validation and API docs, regenerate `openapi.json` and the web types.
- **Settings → Appearance section** (`web/src/lib/settings/sections.ts`): a select for mode and for text size, and the accent as a row of swatch buttons (a new `swatches` control kind in `fields.ts` if the existing ones don't fit). A change previews at once, before saving; discarding the draft reverts it.
- **Applying it:** the root layout sets `data-theme` on `<html>` (`light`, `dark`, or none for system, which the CSS already handles), sets `data-accent`, and sets a `--text-scale` (0.9, 1, 1.15, 1.3) that every `--fs-*` token multiplies, e.g. `--fs-md: calc(14px * var(--text-scale))`. It reapplies when settings are saved.
- **No flash on load:** settings load after the page renders, so the default look would flash first. Cache the applied appearance in `localStorage` and apply it from a small inline script in `web/src/app.html` before first paint; the settings response then corrects it if it changed elsewhere. Wrap storage access in try/catch, so a blocked storage falls back to the default look.
- **Large text must still fit:** at `larger`, the pages (Library, Discover, Queue, series page, Settings, System) have no horizontal scroll at 375px and 1280px wide, and buttons and nav labels don't clip.

Out:
- Named themes (T90) and a custom stylesheet (T91).
- A setting per device or browser.
- Spacing or density options.

## Seams under test
- `fmd-core`: the defaults are `system`/`normal`/`teal`; unknown values are rejected by validation; old settings without the group load with the defaults.
- Web unit tests: applying an appearance sets the right attributes and `--text-scale` on `<html>`; the swatch control saves the picked value.
- Playwright: pick Dark, Large and a different accent; the page's background, `--fs-md` and `--accent` change at once; after a reload they're still applied, with no default-theme frame (check the computed background at `DOMContentLoaded`). At `larger`, `document.documentElement.scrollWidth <= innerWidth` on each page at 375px.

## Acceptance criteria
- [ ] Settings → Appearance sets light/dark/system, text size and accent, previewed live and kept across reloads and devices.
- [ ] Reloading shows the chosen look from the first frame.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
None.
