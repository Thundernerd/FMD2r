# T29: Module updater with hot reload
Deps: T14, T17

## Goal
Keep the Lua tree in sync with upstream (`dazedcat19/FMD2`, path `lua/`, ref `master`) the way FMD2's module updater does (GitHub API with ETag, tree diff, raw downloads), then hot-reload changed modules without restarting, reporting broken modules to the inbox instead of crashing.

## Scope (in/out)
In:
- `ModuleUpdater` job in `fmd-core`:
  1. `GET https://api.github.com/repos/{owner}/{name}/commits?sha={ref}&per_page=1` with `If-None-Match: <stored ETag>`; 304 → nothing to do.
  2. Otherwise `GET …/git/trees/{sha}:{path}?recursive=1`; compare blob SHAs with `module_files`.
  3. Download changed/new blobs from `https://raw.githubusercontent.com/{owner}/{name}/{sha}/{path}/{file}` (bounded concurrency), write atomically (temp + rename) into the lua dir; delete files removed upstream; update `module_files` and stored ETag/sha.
  4. Respect rate limits (`X-RateLimit-Remaining`/`Reset`), optional token from settings.
- Hot reload: after a sync, re-run the module scan (T06) for changed files, swap the `ModuleRegistry` atomically, call `WorkerPool::invalidate` (T14) so workers rebuild states when idle; package cache (`require`d files) invalidated too.
- Reporting: modules that fail `Init` after an update, and modules referencing unknown Host API names (T02/T14 scan), become inbox events (once per file+sha), never a crash; the previous good version stays loaded if a module fails to load (setting: keep-last-good).
- Schedule: at startup and on an interval (settings); `POST /api/modules/update` trigger and `GET /api/jobs/modules` status if T21 is merged (otherwise just the job API).
- First-run bootstrap: if the lua dir is empty, do a full sync.

Out: UI beyond job status (T36 shows it).

## Seams under test
Public `fmd-core` `ModuleUpdater` API against a stub GitHub server (base URLs injectable) and a temp lua dir + temp `app.db`:
- First run: commits 200 with ETag `"e1"`, tree with 3 files → 3 files written, `module_files` has 3 rows.
- Second run: server asserts `If-None-Match: "e1"` and returns 304 → no downloads.
- Third run: tree changes 1 SHA and removes 1 file → 1 download, 1 delete.
- Changed module now has a syntax error → inbox event "module X failed Init" and the registry still contains the previous version (keep-last-good on).
- After update, `WorkerPool` invalidation observed (a callback returns the new module's value).

## Acceptance criteria
- [ ] ETag and tree-diff flow matches `GitHubRepoV3.pas`.
- [ ] Atomic file writes; partial failures leave a consistent tree and retry next run.
- [ ] Hot reload without restart; in-flight jobs finish on the old state.
- [ ] Broken modules reported to the inbox, never crashing the process.

## FMD2 references
- `baseunits/GitHubRepoV3.pas:114-207` (config, ETag `If-None-Match` at :175, commits endpoint at :178, ETag read at :186), `:208-305` (last commit, tree endpoint at :258), `:306-341` (`CheckRateLimited`), `:342-355` (`GetDownloadURL`)
- `mangadownloader/forms/frmLuaModulesUpdater.pas:395-460` (download threads), `:588-684` (`SyncRepos`: diff by SHA), `:685-768` (`Download`), `:769-890` (`DoSync`), `:891-937` (`Execute`)
- `dist/config.json` ("GitHub" block: api_url, download_url, owner, name, ref, path)
- `baseunits/lua/LuaPackage.pas:141-144` (`ClearCache` on reload), `baseunits/lua/LuaWebsiteModules.pas:636-656` (re-scan)
