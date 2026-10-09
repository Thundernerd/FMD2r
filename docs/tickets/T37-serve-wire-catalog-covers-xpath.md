# T37: Wire the module catalog, covers and XPath backend into `serve`
Deps: none

## Goal
`fmd2r serve` builds the worker pool and download engine (T23) but still passes `Idle` for the module catalog and covers, and never applies `xpath.backend`. Against a real server, add-by-URL, the series page, the module pickers and every cover fail. Wire these services into the composition root so the UI works end to end. Found in the merged-PR review (PRs #43, #46, #52, #59).

## Scope (in/out)
In:
- A real `ModuleCatalog` over `LuaRuntime` (`crates/fmd-server/src/module_updates.rs:36`): module list with capabilities, `resolve`, `get_info` on the shared `WorkerPool`, following hot reloads from `LiveModules`. Pass it with `AppState::with_modules` in `crates/fmd-server/src/serve.rs`.
- A real `CoverModules` that returns `fmd_lua::create_http` sessions per module (cookie jar, UA, proxy, connection queue) and the module's `RootURL`. Replace `with_covers(covers, Idle)`.
- Apply the `xpath.backend` setting to the pool's runtimes (`Runtime::set_xpath_backend`) at startup and on setting change (rebuild worker states via `invalidate(All)`).
- If module loading fails, keep serving with `Idle` and log it, as today.

Out: accounts, list jobs and favorites (T38); CI for the real-server run (T39).

## Seams under test
Through `fmd-server`'s public `serve(ServeConfig)` started on a temp data dir with a fixture Lua dir (copy or extend the T23 `tests/tasks_engine.rs` fixture module) and a local stub site:
- `GET /api/modules` lists the fixture module.
- `POST /api/resolve` with a stub-site URL returns `{module_id, link}`; `GET /api/series` returns the fixture's title and chapters.
- `GET /api/covers?module=&url=` returns the stub's image and the stub sees the module's UA and Referer.
- With `xpath.backend = "fpc"` (feature `xpath-fpc` only) vs `"native"`, a fixture callback reports which backend ran (add a small test-only probe if needed).

## Acceptance criteria
- [ ] No `Idle` passed for the catalog or covers when modules load.
- [ ] Add-by-URL → series page → cover works against `fmd2r serve` with the fixture module (checked manually or by T39's run).
- [ ] `xpath.backend` takes effect without a restart.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
- `baseunits/WebsiteModules.pas:353-387` (`PrepareHTTP`/`CreateHTTP`)
- `baseunits/WebsiteModules.pas:500-530` (`LocateModuleByHost`)
- `baseunits/uData.pas:85-208` (`GetInfoFromURL`)
