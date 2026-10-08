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
- `/api/events` is a fake event source. Once a second it advances the downloading tasks (`task.progress`), logs a line per task (`log`) and advances a favorites check (`job.state`). After 30 seconds it posts one `inbox.new` item.
- Add-by-URL accepts a URL with or without `https://`. The mock knows `mangadex.org`, `comick.io`, `bato.to` and `www.webtoons.com`; any other host gets "No module handles this URL".

A mock-mode build goes to `.svelte-kit/build-mock/`, never to `build/`, so it can't be embedded by accident.

## API layer

- `../openapi.json` is the server contract. fmd-server generates it with utoipa: `scripts/export-openapi.sh` writes it (the server also serves it at `/api/openapi.json`). The committed file is still the hand-written seed, because the client already calls `/api/tasks` and `/api/resolve`, which only exist once T23 and T24 land; switch to the generated file then. Run `npm run gen:api` after it changes and commit the regenerated `src/lib/api/schema.d.ts`. Don't hand-write request or response types.
- `src/lib/api/client.ts` wraps the generated `openapi-fetch` client behind the `Api` interface that pages use.
- `src/lib/events.svelte.ts` holds live state from the SSE stream (`task.progress`, `inbox.new`, `job.state`, `log` frames) and reconnects with exponential backoff (1 s doubling to 30 s).
- `src/lib/app.ts` wires both to the real server or to the mock.

## Imports

SvelteKit 3 replaced `$lib` with the `#lib` subpath import (see `imports` in `package.json`). Import TypeScript modules with their extension, e.g. `#lib/api/client.ts`.
