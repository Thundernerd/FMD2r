# T14: Callback runner and worker pool
Deps: T06, T08, T10

## Goal
Run module callbacks the way FMD2 does: on a pool of dedicated OS threads, each owning one Lua state cached per module, with the right globals set before each callback and results read back afterwards. This is the API every feature (downloads, info, list updates, accounts) uses to talk to modules; it is the critical path.

## Scope (in/out)
In:
- `WorkerPool` (in `fmd-lua` or `fmd-core`, your call; keep it free of DB concerns): N OS threads (configurable), each owning a Lua state. A worker reuses its state while jobs target the same module; when it switches modules it rebuilds the state (new state, load the module's cached bytecode, re-run the chunk, push `MODULE`). Globals therefore persist between callbacks of the same module on the same thread.
- Async-friendly submission: `pool.submit(job) -> impl Future<Output = Result<JobResult>>` (channels + oneshot), plus a blocking variant for tests.
- Typed callback API (one method per callback, each sets the documented globals, calls the function by name with no arguments, and reads results back):
  - Update list: `before_update_list`, `get_directory_page_number` (`PAGENUMBER` and `WORKPTR` set as integers; the callback's integer return and `PAGENUMBER` read back), `get_name_and_link(page_index)` (`URL` set to the page index **as a string**; `LINKS`/`NAMES` TStrings read back; `UPDATELIST` object set), `after_update_list`.
  - Info: `get_info(url) -> MangaInfo` (`MANGAINFO` object with `URL` pre-filled; fields `Title`, `AltTitles`, `Link`, `CoverLink`, `Authors`, `Artists`, `Genres`, `Status`, `Summary`, `ChapterNames`, `ChapterLinks`).
  - Download: `task_start`, `get_page_number(chapter_url)` (global `URL` = chapter URL; `TASK` with `PageLinks`, `PageContainerLinks`, `FileNames`, `ChapterLinks`, `ChapterNames`, `CurrentDownloadChapterPtr`, `PageNumber`, `CurrentMaxFileNameLength`, `Link`), `get_image_url(work_id, url)` (`WORKID`, `URL` set), `before_download_image`, `download_image`, `save_image(path, name)`, `after_image_saved(filename)` (with `WORKID`).
  - Accounts: `login`, `account_state`, `check_site`, with globals `no_error`, `net_problem`, `information_not_found` and the `as*` account-status constants set as in FMD2.
  - Return value conventions (boolean/number/nil) mapped exactly as each `Do*` function in `LuaWebsiteModules.pas` does.
- `HTTP` global per callback bound to a session for that module and job (T10), with cancellation wired to the job.
- Full GC (twice, as FMD2 does) every 16 callback invocations per state.
- Module limits exposed to callers: effective `MaxTaskLimit`, `MaxThreadPerTaskLimit`, `MaxConnectionLimit` (module values overridable by settings).
- Hot-reload hook: `pool.invalidate(module_id | all)` marks cached bytecode stale; workers rebuild on next use when idle (used by T29).
- Errors inside callbacks become `Err(CallbackError{module, callback, message, traceback})`, never panics; a worker survives.
- Corpus check: with T02's static scan, report modules referencing Host API names not implemented.

Out: download engine orchestration (T20); persistence.

## Seams under test
Public worker-pool API with fixture modules on disk and a stub HTTP transport, e.g. a module
```lua
function Init() local m = NewWebsiteModule(); m.ID='t'; m.Name='T'; m.RootURL='https://t'; m.OnGetInfo='GetInfo'; m.OnGetNameAndLink='GNL'; m.OnGetPageNumber='GPN' end
calls = 0
function GetInfo() calls = calls + 1; MANGAINFO.Title = 'X'..calls; MANGAINFO.ChapterLinks.Add('/c1'); return no_error end
function GNL() LINKS.Add('/m'..URL); NAMES.Add('n'); return no_error end
function GPN() TASK.PageLinks.Add('p1'); TASK.PageLinks.Add('p2'); return true end
```
- `get_info` twice on a 1-thread pool → titles `X1`, `X2` (globals persist); after a job for another module and back → `X1` (state rebuilt).
- `get_name_and_link(0)` → links `["/m0"]` (URL is the string `"0"`).
- `get_page_number` → 2 page links.
- A callback that raises → `Err` with module id and callback name; the next job on the same worker succeeds.

## Acceptance criteria
- [ ] Globals and read-back per callback match the `Do*` functions in `LuaWebsiteModules.pas`.
- [ ] State caching/rebuild semantics match `LuaWebsiteModuleHandler.pas:33-57`.
- [ ] GC every 16 calls matches `LuaHandler.pas:134-144`.
- [ ] No Lua state crosses threads (`!Send` enforced by types).
- [ ] Unknown-Host-API report generated over the corpus.
- [ ] Doc comments cite Pascal lines.

## FMD2 references
- `baseunits/lua/LuaWebsiteModules.pas:154-465` (`DoBeforeUpdateList` … `DoCheckSite`: globals set, return mapping), `:820-846` (`LuaPushMe`, `LuaPushNetStatus`, `LuaPushAccountStatus`, `LuaDoMe`)
- `baseunits/lua/LuaWebsiteModuleHandler.pas:33-95` (per-thread handler; rebuild when module changes; thread cleanup)
- `baseunits/lua/LuaHandler.pas:42-151` (state lifecycle, `LoadObject`, `CallFunction` with GC every 16 calls at :134-144)
- `baseunits/lua/LuaMangaInfo.pas:18-37`, `baseunits/lua/LuaDownloadTask.pas:18-34`, `baseunits/lua/LuaUpdateListManager.pas:18-42`, `baseunits/lua/LuaMangaCheck.pas:18-35` (object surfaces)
- `baseunits/lua/LuaBase.pas:119-160` (`LuaNewBaseState`, `LuaCallFunction`, return-code strings)
- `baseunits/uBaseUnit.pas:322` (`TMangaInfo` fields)
- `baseunits/WebsiteModules.pas:398-420` (`GetMaxTaskLimit`, `GetMaxThreadPerTaskLimit`, `CanCreateTask`)
- `docs/LUA-REFERENCE.md:276-990` (every callback, injected objects, `PAGENUMBER`, `URL`, return values at :1917-1937)
