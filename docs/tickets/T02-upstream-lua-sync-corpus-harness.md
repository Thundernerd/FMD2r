# T02: Upstream Lua sync script and module corpus test harness
Deps: T01

## Goal
Give the project a reproducible copy of the upstream Lua tree (`dazedcat19/FMD2`, path `lua/`) as a test fixture, and a test harness that later tickets use to run checks over every module (e.g. "every module passes `Init`" once T06 lands).

## Scope (in/out)
In:
- A script (`scripts/sync-upstream-lua.sh` or an `xtask`/`fmd2r` dev subcommand) that fetches `lua/` from `https://github.com/dazedcat19/FMD2` at a given ref (default `master`) into `fixtures/lua/` and writes `fixtures/lua/UPSTREAM_REF` with the commit SHA. Use a sparse/shallow git checkout or the GitHub tarball; no API token required.
- Decide and document whether the fixture is committed or fetched in CI (recommended: commit a pinned snapshot so tests are offline and deterministic; the script refreshes it).
- A corpus harness in a test-support location (e.g. `crates/fmd-lua/tests/corpus/` helper module or a small `fmd-testkit` dev crate) exposing: `corpus_root() -> PathBuf`, `module_files() -> Vec<PathBuf>` (all `lua/modules/*.lua`, sorted), and a helper that runs a closure per module and collects failures into one readable report instead of stopping at the first.
- A static scan helper: list identifiers a module references from the Host API surface (`HTTP.`, `MODULE.`, `MANGAINFO.`, `TASK.`, `require 'fmd.x'`, `CreateTXQuery`, etc.) so later tickets can assert "no unknown Host API names". It returns data; T06/T14 wire it to the runtime.
- A first corpus test that only asserts the fixture is present and contains > 500 module files.

Out: actually loading modules in Lua (T06); module auto-update at runtime (T29).

## Seams under test
- Public functions of the harness crate/module: `module_files()` returns all `*.lua` under `fixtures/lua/modules`, sorted; `scan_host_api_names(source: &str) -> BTreeSet<String>` on a Lua snippet such as `local x = HTTP.GET(u); MODULE.Storage['k'] = 1; require 'fmd.crypto'` returns `{"HTTP.GET", "MODULE.Storage", "fmd.crypto"}`.
- The sync script, run against a local bare git repo fixture (not the network) in a test, populates the target dir and writes `UPSTREAM_REF`.

## Acceptance criteria
- [ ] Running the sync script produces `fixtures/lua/{modules,templates,utils,websitebypass}` and `UPSTREAM_REF`.
- [ ] `cargo test` passes offline.
- [ ] The harness reports all failing modules at once with file names.
- [ ] The scanner handles both dot and colon calls (`HTTP.GET`, `HTTP:GET`) and `require("fmd.x")` / `require 'fmd.x'` forms.

## FMD2 references
- `lua/` (the tree being synced: `lua/modules`, `lua/templates`, `lua/utils`, `lua/websitebypass`, `lua/extras`)
- `dist/config.json` ("GitHub" block: owner `dazedcat19`, name `FMD2`, ref `master`, path `lua`)
- `baseunits/GitHubRepoV3.pas:178`, `:258` (the commits and trees endpoints FMD2 itself uses)
- `docs/LUA-REFERENCE.md:105-1270` (catalogue of injected objects and `fmd.*` libs, the names the scanner should recognise)
