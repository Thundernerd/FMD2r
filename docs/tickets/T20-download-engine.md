# T20: Download engine
Deps: T14, T17, T19

## Goal
The heart of `fmd-core`: a task scheduler and state machine that downloads chapters through module callbacks, persists progress per page, packs output, retries failures and resumes after restart, mirroring `uDownloadsManager.pas`.

## Scope (in/out)
In:
- `DownloadManager` (async, tokio) with API: `add_task(module_id, manga_link, title, chapters[], save_to) -> TaskId`, `start(id)`, `stop(id)`, `start_all`, `stop_all`, `delete(id, delete_files)`, `enable/disable(id)`, `redownload(id)`, `reorder(ids)`, `list()`, `subscribe() -> broadcast::Receiver<EngineEvent>`.
- Statuses: Stopped, Waiting, Preparing, Downloading, Converting, Compressing, Finished, Failed, Disabled, with transitions as in FMD2 (document the transition table in code).
- Scheduling: global max parallel tasks; per-module `MaxTaskLimit` (`CanCreateTask`); threads per task bounded by global setting and module `MaxThreadPerTaskLimit`; connection limit enforced by `fmd-http`.
- Per-chapter pipeline:
  1. `OnTaskStart` once per task (if defined).
  2. `OnGetPageNumber` → page links / container links (unless already known and persisted).
  3. Skip pages whose files already exist; skip the chapter if the packed archive already exists.
  4. `OnGetImageURL` per page unless `DynamicPageLink` is set (then it is called lazily just before each page download).
  5. Download page indices in parallel workers: `OnBeforeDownloadImage` → `OnDownloadImage` or plain GET → `OnSaveImage` or built-in save (extension sniffed from content) → `OnAfterImageSaved`.
  6. Convert images (T19), then pack (T19) to the configured format; name via `CustomRename`.
  7. Mark chapter done in `downloaded_chapters`.
- Persistence: every status/page change written through `TaskRepo` (batched/throttled) so progress survives a crash.
- Recovery: retry failed chapters N times (setting); on startup, tasks that were Downloading/Preparing/Waiting resume (`CheckAndActiveTaskAtStartup`).
- Progress events (bytes, pages done/total, speed) for SSE (T21/T23).
- Cancellation: stop terminates in-flight HTTP and Lua waits promptly.

Out: HTTP API (T23); favorites auto-download trigger (T25).

## Seams under test
Public `fmd-core` `DownloadManager` API with a real `WorkerPool` (T14) loading fixture Lua modules, a stub HTTP transport serving images, a temp `app.db`, and a temp output dir:
- Add a 2-chapter task → events go Waiting→Preparing→Downloading→Compressing→Finished; two CBZs exist with correct page counts; `downloaded_chapters` updated.
- Module with `DynamicPageLink = true` → `OnGetImageURL` called per page lazily.
- Module with `OnDownloadImage`/`OnSaveImage` → custom path used.
- Transport fails page 3 twice then succeeds → task finishes; with retry count 0 → chapter Failed, task Failed.
- Kill mid-download (drop manager), recreate from the same DB → task resumes and skips existing pages.
- `MaxTaskLimit = 1` with two tasks for the same module → second waits.
- Stop during download → status Stopped within a bounded time.

## Acceptance criteria
- [ ] Status transitions and scheduling limits match FMD2 (cite lines).
- [ ] Existing-page and existing-archive skipping works.
- [ ] Resume after restart is tested.
- [ ] No Lua or HTTP work happens on tokio worker threads.
- [ ] Events are emitted for every state change and throttled progress.

## FMD2 references
- `baseunits/uDownloadsManager.pas:19-31` (statuses), `:303-460` (`TDownloadThread`: `DownloadImage` :334, page link :421, download :434)
- `baseunits/uDownloadsManager.pas:461-552` (`TTaskThread.Create`, `GetFileName`), `:553-711` (`Compress`, `Convert`), `:829-881` (`DoGetPageNumber`), `:882-974` (thread/work-id management), `:975-1375` (`TTaskThread.Execute`: full per-chapter pipeline, skipping, retries)
- `baseunits/uDownloadsManager.pas:1376-1499` (`TTaskContainer` status/DB), `:1590-1900` (`TDownloadManager`: Restore, `CheckAndActiveTask` :1784, `CheckAndActiveTaskAtStartup` :1859, Start/Stop :1895-1978)
- `baseunits/lua/LuaWebsiteModules.pas:267-411` (download callbacks)
- `baseunits/WebsiteModules.pas:388-420` (active task counting, limits)
- `baseunits/DownloadedChaptersDB.pas:47-91` (marking downloaded chapters)
