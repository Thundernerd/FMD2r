# End-to-end verification, 2026-10-09

This is the end-to-end check from `docs/plan.md` (Verification → End to end), run against `main` at
`3209133` (Merge #76, T39). That build includes T37, T38, T39, T40–T50 and T52. **Run 2 is the result.** Run 1 is
summarised at the end for history.

## Result: 9 pass / 4 fail

| # | Step | Result |
|---|------|--------|
| 0a | CI on `main` for the latest merge | **FAIL**: the `e2e` job (new in T39) fails ([F1](#f1-ci-e2e-job-fails-on-main)) |
| 0b | Docker workflow on `main` for the latest merge | PASS |
| 1a | `docker compose up --build` from a source checkout | **FAIL** on a host without buildx ([F2](#f2-docker-compose-up---build-fails-without-buildx)) |
| 1b | `/api/health`, SPA loads (phone + desktop) | PASS |
| 2a | Add by URL → series page: title, cover through the cover proxy, metadata, chapter list | PASS |
| 2b | Add to library → shows in the grid | PASS |
| 2c-i | Choose CBZ on the Settings page | **FAIL**: Settings page crashes with "500 Internal Error" ([F3](#f3-settings-page-crashes-duplicate-module-id)) |
| 2c-ii | Queue the 2 earliest chapters; Queue page and bottom dock update live over SSE (progress and speed) | PASS |
| 2d | `docker compose restart` mid-download → task resumes, both chapters finish | PASS |
| 2e | Output: 2 `.cbz` files, naming per settings, valid zip, natural page order, page counts match the module | PASS |
| 2f-i | Library "Check now" | PASS |
| 2f-ii | Inbox popover (badge, open, Esc, outside click) | PASS |
| — | Global proxy setting reaches module HTTP | **FAIL** ([F4](#f4-global-proxy-setting-is-never-applied)). Found while setting up the restart test; it isn't a brief step. |

Other findings, none of which failed a step: [O1–O6](#observations).

## Environment

- Host: CachyOS (Linux 7.2.8), Docker 29.8.1, Docker Compose 5.5.1, **no buildx plugin**, x86_64.
- Stack: `compose.yaml` with the FlareSolverr sidecar, plus a scratch override (not committed). Port `18080`, because 8080 was
  taken on the host. `/data` and `/data/downloads` were bind-mounted to fresh empty dirs inside the worktree
  (gitignored). The image was built locally from `3209133`. `.env` started from `.env.example`.
- `/api/about` after start: version 0.1.0, `module_count` 684, `xpath_backend` native, no load failures; python3,
  node, magick and FlareSolverr all ok. `git_revision` was `null` because the local build doesn't pass `FMD2R_GIT_REVISION`.
- Browser: Playwright 1.64, headless Chromium 156. Phone = `devices['iPhone 13']`; desktop = 1440×900.
- Series: *Yume to Koi de wa Tsuriawanai* (MangaDex, completed, content rating safe, 12 chapters):
  `https://mangadex.org/title/30c8bf1c-9063-4a22-a845-e2cead50adea`. Chapters 1 and 2 have 26 and 28 pages
  (`fmd2r module pages` in the container agrees with the MangaDex API).
- Test-harness changes (not product changes):
  - Built from a copy of the Dockerfile with `ARG BUILDPLATFORM=linux/amd64` added at the top (see F2).
  - CBZ was set with `PATCH /api/settings {"output":{"format":"cbz"}}` because the Settings page crashes (F3).
  - Two chapters download in about 3 s here, too fast to restart in the middle. During the download I policed the
    container's ingress to about 180 kB/s with `tc` from a helper container in its network namespace (the global
    proxy setting would have been the cleaner route but does nothing, see F4). The rule is lost on restart because
    the container gets a new netns, so after the restart the download ran at full speed.

Screenshots and raw results are in [`run2/`](run2/): `results.json` holds the per-step data and SSE/DOM samples,
and `results-dock.json` the dock check. Each step has `-phone.png` and `-desktop.png` screenshots.

## Steps (run 2)

### 0. CI and Docker on `main`

`gh run list --branch main` for `3209133` (Merge #76): **Docker success, CI failure**, with the `e2e` job failing
(`check` and `web` pass). The merges before it (#74 T52, #75 T38) were green. See F1.

### 1. Build, start, health, SPA

- `docker compose up --build` fails at step 1 of the Dockerfile on this host (F2). After the harness change it builds and starts;
  `fmd2r` goes `healthy`. Logs: `seeding /data/lua from the bundled snapshot (ad3a5b6…)`, `listening on 0.0.0.0:8080`.
- `GET /api/health` → `{"status":"ok","auth":false}`. The SPA loads on both viewports and shows the empty Library.
  Screenshot: `01-spa-library-empty`.

### 2a. Add by URL → series page: PASS

I pasted the URL into the top-bar field on the phone and pressed Enter. `POST /api/resolve` → 200, and the page
navigated to `/series?module=d07c9c…&link=/title/30c8bf1c…`. It shows:
- title "Yume to Koi de wa Tsuriawanai", alternative titles, kicker "MANGADEX · Completed";
- author Torii Shiduku, "Chapters 12 · 0 downloaded", genres (Romance, Comedy, Girls' Love, School Life, Safe), summary;
- the cover from `/api/covers?module=…&url=https://uploads.mangadex.org/covers/…256.jpg`, which rendered (`naturalWidth > 0`);
- the chapter list `0001 Vol. 1 Ch. 1 - I Love You, Please Reject Me!` … `0012 Vol. 1 Ch. 8.8 - …`.

Screenshots: `02a-add-by-url-typed-phone`, `02a-series-page`, `02a-series-page-full`.

### 2b. Add to library: PASS

"＋ Add to library" changes to "★ In library". The Library grid shows the series with its proxied cover ("MangaDex ·
Completed"; chips All 1, Completed 1). `GET /api/favorites` returns the favorite with `current_chapter: 12`.
Screenshots: `02b-series-in-library-phone`, `02b-library-grid`.

### 2c. Queue 2 chapters as CBZ, live updates

- **2c-i: FAIL.** `/settings` renders "500 Internal Error" on both viewports (F3), so CBZ was set over the API.
  Screenshot: `02c0-settings-page-error`.
- **2c-ii: PASS.** On the phone series page I selected range `1-2` ("2 chapters selected", Format CBZ) and pressed
  "Download 2 chapters" → `POST /api/tasks` 201, "Queued: … 2 chapters". The phone dock appeared straight away
  (`… 0/26 · 0 B/s`). The desktop Queue page was already open and was never reloaded. It showed the task go from waiting
  to preparing to downloading, with `13/26 pages · 177 kB/s`, the speed graph and "1 active · 0 waiting". The
  phone Queue page showed the same. Over the download the SSE stream carried 30 `task.progress` frames and 8 `task.status` frames.
  To check the dock while something is downloading (it is hidden on `/queue`), I queued chapter 3 separately and watched the Library page
  without reloading. Phone and desktop docks both went `0/38 0 B/s` → `1/38 188 kB/s` → … → `4/38 173 kB/s`.
  After that I stopped and deleted the chapter 3 task.
  Screenshots: `02c-series-two-selected-phone`, `02c-queued-dock`, `02c-queue-live`, `02c-dock-live-1`,
  `02c-dock-live-2`.

### 2d. Restart mid-download: PASS

At `Ch. 001 21/26 pages · 179 kB/s` I ran `docker compose restart fmd2r`. Server log: `shutting down` at 12:05:28.204,
`listening` at 12:05:28.416, and health was back within 0.4 s. `GET /api/tasks/1` right after the restart showed
`status: downloading, current_chapter: 0, total: 26`. The task resumed by itself: chapter 1 finished (pages 22–26
and packing), then chapter 2 went preparing → downloading 0→28 → converting → compressing → finished at 12:05:31.
Final state: `chapters_done: 2`, both chapters `downloaded`, and no `error`. The SSE logger saw the
disconnect at 12:05:28.205 and was getting frames again at 12:05:29.2. Without a reload, the open Queue pages
(phone and desktop) moved the task to "Finished 1" (`2/2 chapters`, "Get files"). A reload showed the same.
Screenshots: `02d-during-restart` (the last state before the stop, 21/26), `02d-finished-no-reload`, `02d-after-reload`.

### 2e. Output files: PASS

```
downloads/Yume to Koi de wa Tsuriawanai/Vol. 01 Ch. 001 - I Love You, Please Reject Me!.cbz   20,942,697 B
downloads/Yume to Koi de wa Tsuriawanai/Vol. 01 Ch. 002 - I Thought of 156 Options.cbz        21,255,613 B
```

- The naming matches the default save settings: manga folder `%MANGA%`, chapter `%CHAPTER%` with
  `convert_digit_volume` (2 digits) and `convert_digit_chapter` (3 digits), illegal characters `posix`. CBZ replaces
  the chapter folder with an archive of the same name.
- `zipfile.testzip()` passes on both. The entries are flat, with no directories.
- Pages: `001.png` … `026.png` and `001.png` … `028.png`, i.e. 26 and 28, matching the module. Zip order equals
  natural order, every entry is a PNG (by magic bytes), and there are no duplicate images (MD5). Entries are stored, not deflated.
- No temporary or partial folder was left for chapters 1–2.

### 2f. Check for new chapters and the inbox: PASS

"Check now" → `POST /api/favorites/check` 202. The progress bar shows, then the `favorites` job ends `done 1/1` with no
error and no alert. There are no new chapters, which is expected for a completed series, so there's no new-chapter inbox item. The inbox
badge shows 2 (two module-loader items, see O1 and O2). The popover opens on both viewports, lists the items with
"Mark read", and closes on Esc (phone) and on an outside click (desktop).
Screenshots: `02f-library-checking`, `02f-library-after-check`, `02f-inbox-open`, `02f-inbox-closed`.

## Failures

### F1: CI `e2e` job fails on `main`

- **Symptom:** run 37925861004 (push of `3209133`), job `e2e`, `web/e2e-real/flow.test.ts:15`:
  ```
  [desktop] Error: locator.click: Error: strict mode violation: getByRole('link', { name: 'Open queue' }) resolved to 2 elements
  [phone]   Error: expect(locator).toContainText(expected) failed — Expected substring: "1/2 chapters" — element(s) not found
  ```
- **Likely cause:** a test bug. After queueing, the series page has two "Open queue" links: the DownloadBox status
  line ("Queued: … Open queue") and the dock's button (visible in `02c-queued-dock`). The phone failure is probably the same
  flow failing later on. In this manual run the product side of that flow passed (2c–2f).
- **Where:** `web/e2e-real/flow.test.ts` (scope the locator to the Download region or the dock).
- **Ticket:** T39 (real-server e2e CI).

### F2: `docker compose up --build` fails without buildx

- **Symptom:** compose falls back to the classic builder (`buildx Docker CLI plugin not found: falling back to the
  classic builder`) and stops at the first stage:
  ```
  Step 1/32 : FROM --platform=$BUILDPLATFORM node:24-trixie-slim AS web
  failed to parse platform : "" is an invalid OS component of "" …: invalid argument
  ```
  `--build-arg BUILDPLATFORM=…` doesn't help: the Dockerfile has no global `ARG BUILDPLATFORM` before the first `FROM`.
  Run 1 (before T49) built fine on the same host.
- **Where:** `Dockerfile:13,22` (`FROM --platform=$BUILDPLATFORM …`), new in T49.
- **Fix options:** declare `ARG BUILDPLATFORM=linux/amd64` (and default `BUILDARCH`/`TARGETARCH`) at the top, or have the
  README say BuildKit/buildx is required.
- **Ticket:** T49 (arm64 native-only images). The README instructions come from T33.

### F3: Settings page crashes (duplicate module ID)

- **Symptom:** `/settings` shows "500 / Internal Error" on both viewports, so no setting can be changed in the UI. Console:
  ```
  Error: https://svelte.dev/e/each_key_duplicate
      at Pe (/_app/immutable/chunks/cYvza6sx.js:1:2356) …
  ```
- **Cause:** upstream `lua/modules/Manga1001.lua:18-19` registers two websites under one ID:
  ```lua
  AddWebsiteModule('1d09f3bea8f148fa9e9215fc578fedcd', 'https://manga1001.win')
  AddWebsiteModule('1d09f3bea8f148fa9e9215fc578fedcd', 'https://hachiraw.win')
  ```
  FMD2r keeps both. `GET /api/modules` returns 684 entries with `1d09f3bea8f148fa9e9215fc578fedcd` (both named "HachiRaw")
  twice. The module picker keys its list by ID (`{#each matches as m (m.id)}`), and Svelte throws on the
  duplicate key, which takes down the whole page. Run 1 didn't hit this because `/api/modules` was empty.
- **Where:** `web/src/lib/components/settings/ModuleSettings.svelte:105` (keyed each). Also the catalog/`/api/modules`, which
  exposes duplicate IDs (module registry in `crates/fmd-lua`, list endpoint in `crates/fmd-server`). The IDs need to be
  deduplicated (decide FMD2's rule for a repeated ID) or the UI needs keys that are always unique.
- **Ticket:** T27 (settings page with per-module options) for the crash. T37 (catalog wiring) or T06 (module loader)
  for the duplicate IDs in the catalog.

### F4: Global proxy setting is never applied

- **Symptom:** with `connections.proxy = {enabled: true, type: http, host: throttle, port: 18888}` stored (`PATCH
  /api/settings` 200), and also after a container restart, `POST /api/resolve` and a full chapter download made no
  connections through the proxy (the proxy logged 0 tunnels; a `curl -x` through it from the same container worked).
- **Cause:** `fmd_http::Client::set_default_proxy` (`crates/fmd-http/src/client.rs:174`) has no callers outside tests.
  The runtime HTTP client is built with `HttpClient::new()` (`crates/fmd-lua/src/pool/host_api.rs:46`) and keeps its
  built-in defaults, because nothing copies `settings.connections` into it. The other setters,
  `set_default_user_agent`, `set_default_retry_count` and `set_default_timeout`, have no callers either, so the
  connection settings for user agent, retry count and timeout probably don't take effect. I only verified this for the proxy.
- **Where:** `crates/fmd-server/src/serve.rs` (wiring) and `crates/fmd-http/src/client.rs`.
- **Ticket:** T09 lists "global default plus per-session override" for the proxy, and T18 the setting. The serve wiring is
  the same gap T37/T38 closed for other services, so it most likely belongs in a follow-up to T37.

## Observations

- **O1. False "MangaPlus.lua failed Init" inbox error.** `lua_pcall: runtime error: /data/lua/modules/MangaPlus.lua:99:
  /data/lua/.fmd2r-staging/modules/MangaPlus.proto: No such file or directory`. The live catalog loads MangaPlus fine
  (`load_failures: []`). The module updater's staging validation runs the live file but resolves the module's sibling
  `MangaPlus.proto` against `.fmd2r-staging`, where it was never staged. Ticket T29 (module updater) or T46 (hot reload).
- **O2. Inbox bodies are raw JSON**, e.g. `{"file":"modules/OrckuMangas.lua","names":["MANGAINFO.Artist"]}` and an
  escaped Lua stack trace (`02f-inbox-open`). Ticket T29 (the item text) or T22 (popover).
- **O3. Adding to the library marks every current chapter "✓ downloaded".** This is deliberate FMD2 parity
  (`crates/fmd-server/src/favorites.rs:159`, `btAddToFavoritesClick`), but the series page then says "2 downloaded
  before" for chapters that were never downloaded, and "Hide downloaded" hides everything (`02c-series-two-selected-phone`).
  The wording is worth revisiting. Ticket T24/T25.
- **O4. Stale total in the first frame of a new chapter.** After the restart, the first `task.progress` for chapter 2 was
  `"chapters":"Vol. 01 Ch. 002 … (2/2)","status":"preparing","done":0,"total":26` (chapter 1's total), then 28. This is cosmetic.
  Ticket T20/T23.
- **O5. The server logs nothing about task lifecycle at `info`.** `/api/logs` and `docker compose logs` hold only `listening`,
  `shutting down` and `listening` across the whole run. The resume after the restart and the downloads leave no trace.
  Ticket T48 (logs) or T20.
- **O6. Stopping a task keeps its partial chapter folder** (`Vol. 01 Ch. 003 - Please Forgive Me/001…004.png`). This
  matches FMD2, so I'm only noting it.

## Run 1 (history)

Run 1 used `8175d52` (before T37/T38). The build, `/api/health`, SPA load, saving CBZ in Settings and the inbox popover passed.
**Everything from 2a on failed** because `fmd2r serve` didn't wire the module catalog, covers, accounts, list jobs
or favorites into the app state (`crates/fmd-server/src/serve.rs` never called `.with_modules` and passed `Idle` to
`.with_covers`):
- `/api/about` showed `module_count: 0`;
- `POST /api/resolve` → 404 "no module handles …", so the UI said "No module handles this URL.";
- `/api/series` and `POST /api/favorites` → 404 "no such module";
- `POST /api/favorites/check` → 503, so the UI said "The new-chapter check is not running on this server.";
- `/api/covers` → 404.

The run was paused until T37 and T38 (and T39) were merged. Its evidence is in [`run1/`](run1/).
