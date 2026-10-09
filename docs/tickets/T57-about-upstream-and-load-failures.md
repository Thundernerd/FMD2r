# T57: `/api/about`: upstream ref and sha, and load failures
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification. T36 specifies "upstream Lua ref/sha" and the load failures in `GET /api/about`, and `ModulesReport` has the fields (`crates/fmd-server/src/services.rs:180-190`), but the real catalog never fills them: `LuaCatalog::report` (`crates/fmd-server/src/lua_catalog.rs:75-87`) sets `module_count` and `xpath_backend` and takes the rest from `ModulesReport::default()`. So `upstream_ref` and `upstream_sha` are always `null`, and `load_failures` is always `[]` even when a module fails to load. The data exists: `/data/lua/UPSTREAM_REF` holds the bundled snapshot's sha, the updater knows the synced commit (`ModuleUpdater::synced_commit`, `crates/fmd-core/src/module_updater.rs:339-343`) and the followed ref (`settings.modules.repo_ref`, `crates/fmd-core/src/settings/model.rs:447`), and the loader returns its failures (`crates/fmd-lua/src/module/loader.rs:30`).

## Scope (in/out)
In:
- `upstream_ref`: the followed ref.
- `upstream_sha`: the synced commit, else the `UPSTREAM_REF` of the seeded snapshot.
- `load_failures`: the failures of the registry that is live now (updated on hot reload).

Out: the System page layout (it already shows these fields).

## Seams under test
Through `fmd-server`'s public `serve(ServeConfig)` on a temp data dir with a fixture Lua dir holding an `UPSTREAM_REF` and one module that fails `Init`: `GET /api/about` returns the ref, that sha, and one load failure naming the file.

## Acceptance criteria
- [ ] `/api/about` on a fresh Docker start shows the snapshot's sha and the followed ref.
- [ ] A module that fails to load shows up in `load_failures`.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
- `mangadownloader/forms/frmLuaModulesUpdater.pas` (the synced commit)
