# T90: Built-in themes
Deps: T89

## Goal
Beyond light/dark, text size and accent (T89), offer a few complete looks to pick from in Settings → Appearance. Each one is a different set of token values in `web/src/lib/styles/tokens.css`, with the layout staying the same.

## Scope (in/out)
In:
- **`appearance.theme`** in the settings group from T89: `default` (today's look), `high-contrast`, `warm` (paper-like neutrals and a serif display font) and `compact` (the next smaller `--fs-*` step and tighter `--sp-*` spacing, for more rows on screen). Add validation, regenerate `openapi.json` and the web types.
- **Each theme is a CSS block** keyed on an attribute, e.g. `:root[data-style='high-contrast']`, overriding only the tokens it changes. Each one also has a dark variant, so `mode` (T89) still works with every theme.
  - `high-contrast`: text and borders at least 7:1 against their background, focus outline 3px.
  - `compact`: may tighten spacing tokens, but doesn't change widths or breakpoints.
- **Accent with a theme:** the accent swatch from T89 overrides the theme's accent, except in `high-contrast`, which keeps its own and greys out the swatches with a note saying why.
- **Settings → Appearance:** a theme picker of small preview cards (background, surface, text and accent of the theme) above the existing controls. Picking one previews live, like the other appearance settings, and goes through the same no-flash path as T89.
- **Fonts:** a theme that needs another font bundles it through `@fontsource`, like the current ones (`web/src/lib/styles/app.css:1-8`). No fonts from the network at run time.

Out:
- Themes made by users (T91 covers custom CSS).
- Per-page themes.

## Seams under test
- `fmd-core`: `theme` defaults to `default`; unknown values are rejected.
- Web unit test: each theme value sets `data-style`; in `high-contrast` the accent swatches are disabled.
- A contrast test over `tokens.css`: for every theme × mode, `--fg` on `--bg` and `--surface`, `--muted` on `--surface`, and `--accent-fg` on `--accent` meet the theme's minimum (4.5:1, or 7:1 for `high-contrast`).
- Playwright: picking each theme changes `--bg`; after a reload the theme is applied from the first frame; `compact` and `warm` leave no horizontal scroll at 375px on Library and Discover.

## Acceptance criteria
- [ ] Settings → Appearance offers Default, High contrast, Warm and Compact, each in light and dark.
- [ ] Every theme meets its contrast minimum.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
None.
