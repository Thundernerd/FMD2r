# Build the web UI as a SvelteKit SPA embedded in the Rust binary

FMD2r's web UI is a Svelte 5 / SvelteKit single-page app (`adapter-static`, no SSR) talking to the Rust (axum) server over a JSON REST API, with one multiplexed Server-Sent Events stream for live updates. The built assets are embedded in the binary (`rust-embed`), so FMD2r still ships as one self-contained executable. We chose a JS frontend in a Rust project because the Library layout is client-heavy: a live queue panel docked on every page, virtualised cover grids, and instant facet filtering. The Svelte ecosystem handles that more maturely, with smaller bundles and faster UI iteration, than the Rust/WASM frameworks or server-rendered HTML.

## Considered Options

- **Rust full-stack WASM (Leptos/Dioxus)**: one language and shared types, but heavier bundles, slow UI rebuilds, a thin component ecosystem (virtual lists, accessible primitives) and unstable framework APIs.
- **Server-rendered HTML + htmx**: no JS build, but instant facet filtering and virtualised grids need either a server round-trip per interaction or hand-written JS anyway.
- **Serving assets from a directory beside the binary**: simpler release builds, but the binary is no longer complete on its own and binary and UI versions can drift.

## Consequences

- Node (pnpm) is needed to **build** the frontend, never at runtime. The frontend build is a separate step before `cargo build`, never run from `build.rs`, so Rust-only work and module-compatibility CI don't need Node. `--ui-dir` serves assets from disk for development.
- The Rust/TypeScript boundary is an **OpenAPI spec generated from the handlers** (`utoipa` → `openapi-typescript`/`openapi-fetch`). CI fails when the checked-in spec or the generated client is stale.
- Commands go over REST only. The SSE stream is one-way, and clients resync by refetching a snapshot on every (re)connect.
