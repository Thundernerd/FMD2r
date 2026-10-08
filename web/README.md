# web

The FMD2r web UI: a SvelteKit (Svelte 5, TypeScript strict) single-page app built with `@sveltejs/adapter-static`. `npm run build` writes it to `build/`, which `fmd-server` (T21) embeds and serves with `index.html` as the fallback for every route.

Layout follows prototype variant B "Library" (https://claude.ai/artifact/As3XTN7NjxM8ydAP9oEcqL): a top bar with the main nav, the add-by-URL field and the inbox, and a queue dock at the bottom. Styles are mobile-first: phones get a bottom tab bar, and from 861px up the main nav moves into the top bar.

## Scripts

| Command            | What it does                                                                                     |
| ------------------ | ------------------------------------------------------------------------------------------------ |
| `npm run dev`      | Dev server; `/api` is proxied to `fmd2r serve` at `$FMD2R_URL` (default `http://127.0.0.1:8080`) |
| `npm run dev:mock` | Dev server in mock mode (no backend needed)                                                      |
| `npm run build`    | Production build into `build/`                                                                   |
| `npm run lint`     | Prettier check and ESLint                                                                        |
| `npm run check`    | `svelte-check` type checking                                                                     |
| `npm test`         | Vitest unit tests                                                                                |
| `npm run test:e2e` | Playwright smoke tests against `vite preview` in mock mode                                       |
| `npm run gen:api`  | Regenerate `src/lib/api/schema.d.ts` from `../openapi.json`                                      |

## Mock mode

Set `VITE_API_MOCK=true` (or run `npm run dev:mock`) to run the UI without the backend. The flag is read at build time, so it also applies to `vite build` and `vite preview`:

```sh
npm run dev:mock                                  # dev server
VITE_API_MOCK=true npm run build && npm run preview  # production build
```

In mock mode `src/lib/api/mock.ts` stands in for fmd-server:

- `/api/*` requests are answered from in-memory data (inbox items, tasks) that resets on reload. Marking an inbox item read sticks until then.
- `/api/events` is a fake event source. Once a second it advances the downloading tasks (`task.progress`), logs a line per task (`log`) and advances every running job (`job.state`). After 30 seconds it posts one `inbox.new` item.
- The System page's endpoints are mocked too: `/api/logs` starts with a few dozen lines, `/api/jobs` has three jobs (a running favorites check, an idle list update and a failed module update) that `POST /api/jobs/{id}/run` and `/cancel` control, and `/api/about` reports `magick` missing, FlareSolverr unreachable and one module load failure linked to an inbox item.
- The Settings page's endpoints live in `src/lib/api/mock-settings.ts`: `/api/settings` starts from the server's defaults and checks the same ranges, `/api/modules` lists four modules (MangaDex declares one option of each kind but edit, ComicK an edit option), and `/api/preview-rename` substitutes the tokens without FMD2's padding or symbol rules. Saved settings are kept in `sessionStorage`, so they survive a reload.
- Add-by-URL accepts a URL with or without `https://`. The mock knows `mangadex.org`, `comick.io`, `bato.to` and `www.webtoons.com`; any other host gets "No module handles this URL".

A mock-mode build goes to `.svelte-kit/build-mock/`, never to `build/`, so it can't be embedded by accident.

## API layer

- `../openapi.json` is the server contract. fmd-server generates it with utoipa: `scripts/export-openapi.sh` writes it (the server also serves it at `/api/openapi.json`). The committed file is still the hand-written seed, because the client already calls `/api/tasks` and `/api/resolve`, which only exist once T23 and T24 land; switch to the generated file then. Until then, copy the paths and schemas of endpoints the server does serve from the generated document into the seed (T36 did so for `/api/logs`, `/api/jobs*` and `/api/about`, T27 for `/api/settings`, `/api/preview-rename` and `/api/modules*`, T31 for `/api/accounts*`, T25 for `/api/favorites*`). Run `npm run gen:api` after it changes and commit the regenerated `src/lib/api/schema.d.ts`. Don't hand-write request or response types.
- `src/lib/api/client.ts` wraps the generated `openapi-fetch` client behind the `Api` interface that pages use.
- `src/lib/events.svelte.ts` holds live state from the SSE stream (`task.progress`, `inbox.new`, `job.state`, `log` frames) and reconnects with exponential backoff (1 s doubling to 30 s).
- `src/lib/app.ts` wires both to the real server or to the mock.

## Imports

SvelteKit 3 replaced `$lib` with the `#lib` subpath import (see `imports` in `package.json`). Import TypeScript modules with their extension, e.g. `#lib/api/client.ts`.
