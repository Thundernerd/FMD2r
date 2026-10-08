# T23: Queue: API and page
Deps: T20, T21, T22

## Goal
Let the user see and control downloads: REST endpoints over the download engine, live progress via SSE, and the Queue page (status groups, history filter, speed graph, task actions, "Get files").

## Scope (in/out)
In:
- Endpoints (OpenAPI-documented): `GET /api/tasks?status=&q=&page=`, `GET /api/tasks/{id}` (with chapters and per-chapter progress), `POST /api/tasks` (add: module, manga link, title, chapter selection, save-to override), `POST /api/tasks/{id}/start|stop|redownload|enable|disable`, `POST /api/tasks/start-all|stop-all`, `DELETE /api/tasks/{id}?files=true|false`, `POST /api/tasks/reorder`, `DELETE /api/tasks?status=finished` (remove finished), `GET /api/tasks/{id}/files` (streams the CBZ/archive, or a zip of multiple chapter files / folder output, with `Content-Disposition`).
- Engine events forwarded to `/api/events` as `task.status` / `task.progress` (throttled, e.g. ≤ 4/s per task).
- Queue page: groups (Downloading, Waiting, Stopped/Failed, Finished) with counts; history filter (date range, text, status); per-task row with progress bar, speed, pages, chapter; actions (start, stop, retry, delete with/without files, open series, Get files); aggregate speed graph (last N minutes from SSE); `QueueDock` uses the same store.
- Playwright smoke test for the page against the mock API, and one against a real `fmd2r serve` with fixture modules if feasible.

Out: engine internals (T20).

## Seams under test
- HTTP handlers via `build_router` + `oneshot`, with a fake `DownloadManager` behind the engine trait (assert calls) and a second test with the real engine on fixtures: add task → `GET /api/tasks/{id}` shows it; stop → status Stopped; `GET …/files` streams bytes with `application/vnd.comicbook+zip` (or `application/zip`).
- Frontend: Vitest on the queue store (grouping, filtering, speed aggregation from progress events); Playwright smoke: tasks render in groups, stop button calls the API, Get files triggers a download.

## Acceptance criteria
- [ ] All endpoints in OpenAPI and the generated client.
- [ ] Live progress visible without polling.
- [ ] Get files works for cbz/zip/pdf/epub and folder outputs.
- [ ] Mobile layout usable at 375px.

## FMD2 references
- `baseunits/uDownloadsManager.pas:1769-2045` (manager operations exposed as actions: add, start/stop, redownload, delete, remove finished, enable/disable)
- `baseunits/uDownloadsManager.pas:2046-2099` (sort orders)
- `mangadownloader/forms/frmMain.pas:1995-2041` (download info refresh timers)
