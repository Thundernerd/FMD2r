# T36: System page: logs, background jobs, about
Deps: T21, T22

## Goal
A System page for operating the server: live logs, background job status and controls, and an about/diagnostics panel.

## Scope (in/out)
In:
- Endpoints: `GET /api/logs?level=&module=&since=&limit=` (from the in-memory ring buffer, optionally persisted), `GET /api/jobs` (favorites check, list updates, module updater, plus any registered job: state, last run, next run, progress, last error), `POST /api/jobs/{name}/run`, `POST /api/jobs/{name}/cancel`, `GET /api/about` (version, git revision, upstream Lua ref/sha and module count, load failures count, XPath backend, data dir, DB sizes, uptime, tool availability: python3/node/magick/FlareSolverr reachability).
- Live log lines via the existing SSE `log` events.
- System page: tabs/sections Logs (virtualised list, level filter, module filter, search, pause/follow, copy), Jobs (cards with progress and run/cancel buttons, last error expandable), About (diagnostics table, module load failures list linking to inbox items).
- Job registry trait in `fmd-core` so jobs from T25/T26/T29 register themselves; this ticket can ship with fake jobs if those aren't merged.

Out: job implementations.

## Seams under test
- HTTP handlers via `oneshot` with a fake job registry: `GET /api/jobs` lists two fake jobs; `POST /api/jobs/x/run` calls the fake; unknown job → 404; `GET /api/logs?level=warn` filters; `GET /api/about` includes version and tool checks (tool probes behind a trait, faked).
- Frontend: Vitest on log filtering/follow logic; Playwright smoke on mock API: logs stream in, job run button shows progress.

## Acceptance criteria
- [ ] Logs filterable and live; follow mode doesn't jank with 10k lines.
- [ ] Jobs controllable from the UI.
- [ ] About shows everything listed, with failed tool checks highlighted.

## FMD2 references
- `mangadownloader/forms/frmLogger.pas` (FMD2 log window)
- `mangadownloader/forms/frmCheckModules.pas` (module check/diagnostics)
- `mangadownloader/forms/frmLuaModulesUpdater.pas:938-951` (status reporting for module updates)
