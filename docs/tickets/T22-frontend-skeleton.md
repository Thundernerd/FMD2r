# T22: Frontend app skeleton
Deps: T01

## Goal
Create the SvelteKit SPA in `web/` with the "variant B — Library" layout from the approved prototype (https://claude.ai/artifact/As3XTN7NjxM8ydAP9oEcqL): routing, design tokens, shared chrome (top nav, inbox popover, add-by-URL field, bottom queue dock), an SSE store, and a typed API layer that runs against a mock API until T21 lands.

## Scope (in/out)
In:
- SvelteKit + Svelte 5 (runes) + TypeScript `strict`, `@sveltejs/adapter-static` with SPA fallback (`index.html`), Vite, ESLint + Prettier, Vitest for unit tests, Playwright for smoke tests.
- Routes (placeholder content): `/` Library, `/series/[module]/[...link]` Series, `/discover`, `/queue`, `/settings`, `/system`.
- Design tokens (CSS custom properties: colours incl. dark mode, spacing, radii, typography) taken from the prototype; mobile-first responsive layout; bottom navigation on phones, top nav on desktop.
- Shared chrome components: `TopNav`, `InboxPopover` (list + unread badge + mark read), `AddByUrl` field (submits a URL, navigates to the series page), `QueueDock` (collapsed summary of active tasks with progress).
- API layer: `src/lib/api/` with a client interface; a generated client from OpenAPI (script `npm run gen:api` reading `../openapi.json` with `openapi-typescript` + `openapi-fetch`, or similar) and a **mock implementation** (MSW or an in-memory fake) selected by env var, so the UI works with `npm run dev` without the backend.
- SSE store: `events.svelte.ts` connecting to `/api/events`, reconnecting with backoff, exposing typed reactive state (tasks progress map, inbox items, job states, log lines); fed by a fake event source in mock mode.
- `npm run build` outputs to `web/build` for embedding by T21.
- CI job: install, lint, type-check (`svelte-check`), unit tests, build.

Out: page contents (T23–T27, T36).

## Seams under test
- Vitest unit tests on the SSE store: given a fake `EventSource` emitting `task.progress` and `inbox.new` frames, the store's reactive state updates; on error it reconnects with backoff.
- Vitest on the API client wrapper against the mock: `listInbox()`, `markRead(id)`.
- One Playwright smoke test (against `vite preview` in mock mode): app loads, nav reaches each route, inbox popover opens, add-by-URL navigates to the series route.

## Acceptance criteria
- [ ] `npm ci && npm run lint && npm run check && npm test && npm run build` pass in CI.
- [ ] Layout matches prototype variant B at phone (375px) and desktop widths (screenshot in PR).
- [ ] TypeScript strict, no `any` without justification.
- [ ] Mock mode documented in `web/README.md`.

## FMD2 references
- None (new UI). For which information each screen eventually needs, see `mangadownloader/forms/frmMain.lfm` (FMD2's main window tabs: downloads, website/manga list, favorites, options).
