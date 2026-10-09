# E2E verification 2026-10-09: paused notes (run 1)

Paused by the user: the failures from 2a on are expected until T37 and T38 land (PR #60).

## Setup (resume with this)
- Stack: `e2e-run/dc.sh` (compose project `fmd2r-e2e`, `compose.yaml` plus `e2e-run/compose.override.yaml`,
  env `e2e-run/.env`). Port **18080**, because 8080 is taken by a local fmd2r. Image tag `fmd2r:e2e-local`.
- Data is bind-mounted at `e2e-run/data` and downloads at `e2e-run/manga`. `/e2e-run/` is excluded via `.git/info/exclude`.
- The output format is already set to CBZ in the settings (set through the UI, run 1).
- Playwright 1.64 is installed in the session scratchpad. Copies of the driver scripts: `e2e-run/e2e.js` (full UI flow)
  and `e2e-run/e2e2.js` (the 2a error capture plus API-queued 2c/2d; never run). On resume, rerun `e2e.js` against a rebuilt image.
- Series: "Yume to Koi de wa Tsuriawanai", https://mangadex.org/title/30c8bf1c-9063-4a22-a845-e2cead50adea
  (completed, safe). MangaDex module id `d07c9c2425764da8ba056505f57cf40c`.
  Ch 1 `/e1023b89-908f-45df-9d47-8d6e1adc66ed` = 26 pages; Ch 2 `/14c8e4ed-adc4-4cf8-8546-4c1192c0592d` = 28 pages
  (from `fmd2r module pages` inside the container).

## Step 0: CI and Docker on main
- When checked, the CI and Docker runs for merge #59 (8175d52) were still in progress. The previous merges (#57, #58) were green. Check again on resume.

## Run 1 results (`e2e-run/results-run1.json`, screenshots in `docs/verification/2026-10-09/`)
| Step | Result |
|---|---|
| 1 health + SPA | PASS: `/api/health` returns `{"status":"ok"}`, the container is healthy, and the SPA loads at both viewports |
| 2a add by URL | FAIL: `POST /api/resolve` returns 404 "no module handles …"; `/api/about` shows `module_count: 0` |
| 2b add to library | BLOCKED by 2a; `POST /api/favorites` returns 404 "no such module" |
| CBZ setting | PASS: saved through the Settings page |
| 2c–2e queue, restart, files | NOT RUN (paused) |
| 2f check for new chapters | FAIL: `POST /api/favorites/check` returns 503; the UI shows "The new-chapter check is not running on this server." |
| 2f inbox popover | PASS: badge shows 1; the popover closes on Esc and on an outside click |

## Findings
- Root cause of 2a/2b/2f: in `crates/fmd-server/src/serve.rs`, `serve` never calls `.with_modules`, `.with_favorites`,
  `.with_accounts`, `.with_list_jobs` or `.with_jobs`, and it passes `Idle` to `.with_covers` (so `/api/covers` returns 404).
  The download engine *is* given the live modules, so the API path for 2c–2e should work. The user says T37 and T38 cover this.
- Minor: the inbox item body is raw JSON (`{"file":"modules/OrckuMangas.lua","names":["MANGAINFO.Artist"]}`).
- Minor: `fmd2r module info` prints raw chapter links (bare UUIDs). `module pages` therefore needs `https://mangadex.org/<uuid>`;
  `https://mangadex.org/chapter/<uuid>` silently returns `page_links: ["W"]`.
- Logs: `e2e-run/fmd2r-logs-run1.txt`; the SSE capture is in `e2e-run/sse.log`.

## Partial run 2 (interrupted)
`e2e2.js` started before the pause and was killed while in 2c. Results so far, in `e2e-run/results-run2.json`:
- 2a: the error screenshots were captured (`02a-add-by-url-error-*`). The UI shows "No module handles this URL."
- 2c: the task was queued over `POST /api/tasks` (the workaround) and completed. Both chapters landed in
  `e2e-run/manga/Yume to Koi de wa Tsuriawanai/` as `Vol. 01 Ch. 001 - I Love You, Please Reject Me!.cbz` and
  `Vol. 01 Ch. 002 - I Thought of 156 Options.cbz`. These contents are not verified yet: zip validity, page order, and
  the page counts against 26/28. The restart test (2d) did not run.
- On resume, delete `e2e-run/manga/*` and the task, or start from a fresh data dir, so the next run starts clean.
