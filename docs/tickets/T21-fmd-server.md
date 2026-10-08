# T21: `fmd-server`: axum, OpenAPI, SSE, optional auth, embedded SPA
Deps: T17

## Goal
The HTTP server skeleton every UI feature plugs into: axum app with OpenAPI generation, one SSE event stream, optional single-user auth, static SPA serving from the binary, and `fmd2r serve` wiring.

## Scope (in/out)
In:
- `fmd-server` crate: `build_router(state: AppState) -> Router` and `serve(config) -> Result<()>`.
- `AppState` holding store handles, settings service, an event bus (`broadcast`), and trait objects for engine/jobs that later tickets fill (use traits so this ticket doesn't depend on T20).
- OpenAPI via `utoipa` served at `/api/openapi.json` (+ optional Swagger/Scalar UI in debug). A `cargo xtask`/script exports the JSON so `web/` can generate its client.
- `GET /api/events`: SSE stream of typed events (`task.progress`, `task.status`, `job.*`, `inbox.new`, `log`), with heartbeat comments and `Last-Event-ID` best-effort resume from the `events` table.
- Initial endpoints: `GET /api/health`, `GET/PATCH /api/settings` (T18 model; if T18 isn't merged yet, keep the handler behind the trait), `GET /api/inbox`, `POST /api/inbox/{id}/read`, `GET /api/logs?since=` (tail of an in-memory ring buffer fed by `tracing`).
- Optional auth: if a password/token is configured, require `Authorization: Bearer <token>` or a session cookie obtained via `POST /api/login`; constant-time compare; no user accounts. Health stays public.
- Embedded SPA via `rust-embed` from `web/build` with SPA fallback to `index.html` for non-`/api` paths; a placeholder page when the build is absent.
- Error mapping: domain errors → JSON problem responses with proper status codes.
- `fmd2r serve --bind 0.0.0.0:8080 --data-dir DIR` starts it (anyhow in the binary).

Out: feature endpoints (T23–T28, T36).

## Seams under test
HTTP handlers through `build_router` with `tower::ServiceExt::oneshot` (no real socket) and a temp store:
- `GET /api/health` → 200.
- With auth configured: `GET /api/inbox` without token → 401; with token → 200; `POST /api/login` with wrong password → 401.
- `GET /api/openapi.json` → valid OpenAPI 3.1 containing the declared paths.
- `GET /api/events`: publish an event on the bus → client receives a matching `data:` frame.
- `GET /some/spa/route` → `index.html` content; `GET /api/unknown` → 404 JSON.

## Acceptance criteria
- [ ] Router, OpenAPI, SSE and auth implemented and tested at the handler level.
- [ ] OpenAPI JSON exportable to a file for the frontend.
- [ ] SPA embedding works with and without a built `web/`.
- [ ] `fmd2r serve` starts and shuts down gracefully on SIGINT/SIGTERM.

## FMD2 references
- None directly (FMD2 is a desktop app). For event kinds, see the UI updates driven by `baseunits/uDownloadsManager.pas:741-803` (balloon hints), `mangadownloader/forms/frmMain.pas:2015` (`tmRefreshDownloadsInfoTimer`), `mangadownloader/forms/frmLogger.pas` (log view).
