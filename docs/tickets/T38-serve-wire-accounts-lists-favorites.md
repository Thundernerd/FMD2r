# T38: Wire accounts, list jobs and the favorites checker into `serve`
Deps: T37

## Goal
`AccountService` (T31), `ListJobs` (T26) and `FavoritesChecker` (T25) are only reachable in tests: `serve` never calls `with_accounts`, `with_list_jobs` or `with_favorites`, so the Accounts section is empty, "Update list"/"Get from FMD2-DB" return 503, and "Check for new chapters" returns 503 with no scheduled checks. Found in the merged-PR review (PRs #48, #54, #56).

## Scope (in/out)
In:
- Build `AccountService` over the shared pool and `StoreModuleSettings` (accounts key file `ACCOUNTS_KEY_FILE` in the data dir) and pass it with `with_accounts`.
- Build `ListJobs` over the pool, `lists.db` and the settings' `db_url`; register it with the job registry; pass it with `with_list_jobs`.
- Build `FavoritesChecker` over the pool, the `DownloadManager` (as `TaskQueue`) and settings; register the `favorites` job; start `schedule()` (startup check + interval).
- All three follow hot reloads from the module updater.

Out: anything already wired by T37.

## Seams under test
Through `serve(ServeConfig)` on a temp data dir with a fixture module and stub site:
- `PUT /api/accounts/{module}` then `POST …/login` returns Valid for the fixture's credentials.
- `POST /api/lists/{module}/update` returns 202 and the job finishes with the fixture's titles searchable through `GET /api/lists/search`.
- `POST /api/favorites` then `POST /api/favorites/check` finds the stub's new chapter (inbox item, or a task when auto-download is on).
- `GET /api/jobs` lists `modules`, `favorites` and the list jobs.

## Acceptance criteria
- [ ] No 503 from the accounts, lists or favorites endpoints when modules load.
- [ ] Scheduled favorites check runs at startup when the setting is on.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
- `mangadownloader/forms/frmAccountManager.pas:130,299`
- `baseunits/uUpdateThread.pas`, `baseunits/DBUpdater.pas`
- `baseunits/uFavoritesManager.pas`, `mangadownloader/forms/frmMain.pas:1871-1878`
