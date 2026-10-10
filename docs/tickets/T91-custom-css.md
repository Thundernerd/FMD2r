# T91: Load a custom stylesheet from the data folder
Deps: T88

## Goal
For changes the Appearance settings (T89, T90) don't offer, let the user drop a `custom.css` into FMD2r's data folder. The web UI loads it after its own styles, so it can override any token in `web/src/lib/styles/tokens.css` (or any class) without rebuilding the app.

## Scope (in/out)
In:
- **`GET /custom.css`** on the server, routed before the SPA fallback (`crates/fmd-server/src/spa.rs:45`, `crates/fmd-server/src/lib.rs:180`). It reads `custom.css` from the app data directory (`AppState::data_dir`, `crates/fmd-server/src/state.rs:54`) on every request, so edits show on the next reload without restarting.
  - Served as `text/css; charset=utf-8` with `Cache-Control: no-cache`.
  - A missing file, or no data directory, is an empty 200 response, not a 404, so the browser console stays clean.
  - Files over 1 MiB get a 413.
  - It doesn't need a login (it's styling, and the login page should use it too).
- **The page loads it last:** `web/src/app.html` gets `<link rel="stylesheet" href="/custom.css">` after `%sveltekit.head%`, so in a production build it comes after the app's CSS and wins at equal specificity. Note in the docs that in `npm run dev` Vite injects styles later, so overrides there may need `!important`.
- **Settings → Appearance** (or Settings → General until T89 lands) says, in one line of help, that `custom.css` in the data folder is loaded, with the full path as `GET /api/about` reports it (`data_dir`, `crates/fmd-server/src/about.rs:69`).
- **Docs:** a short section in `README.md` with an example that changes the font size, the accent and the background:
  ```css
  :root { --fs-md: 16px; --accent: #8a3ffc; }
  :root[data-theme='dark'] { --bg: #000; }
  ```
  It also lists the tokens worth overriding, and says that class names aren't a stable API but tokens are.
- In Docker, the data folder is already a volume, so the file lives next to the databases. Mention that in the README section.

Out:
- Editing the CSS from the UI.
- Custom JavaScript.
- Serving other files (fonts, images) from the data folder. `@import` of an external URL from `custom.css` works on its own.

## Seams under test
- `fmd-server` route tests: with a `custom.css` in the data dir, `GET /custom.css` returns its body as `text/css` with `no-cache`; editing the file changes the next response; with no file, or no data dir, it's an empty 200; a 2 MiB file is a 413; it works without a session when a password is set.
- Playwright (real server config, `web/playwright.real.config.ts`): a `custom.css` setting `--bg` changes the page's computed background, on the login page too.

## Acceptance criteria
- [ ] A `custom.css` in the data folder restyles the UI after a reload, without a restart.
- [ ] No `custom.css` means no errors and the default look.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
None.
