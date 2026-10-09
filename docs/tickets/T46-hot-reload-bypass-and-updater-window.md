# T46: Hot reload for the anti-bot scripts, and the updater's broken-module window
Deps: none

## Goal
Two hot-reload gaps from PRs #44 and #51:
- `checkantibot.lua`/`websitebypass.lua`/`cloudflare.lua` load once per thread, so module-updater syncs and FlareSolverr setting changes only apply after a restart.
- Between the updater writing a broken module file and restoring the last good one, a worker compiling that file for the first time can load the broken version.

## Scope (in/out)
In:
- Invalidate the cached check/bypass states when the updater changes any file under `websitebypass/` or the FlareSolverr setting changes (rewrite `websitebypass_config.json` and reload).
- Make updates atomic from the workers' point of view: validate a new module file (`Init` in a scratch state) before it replaces the live file, or stage the new tree and swap it in, so workers never see an unvalidated file.

Out: changing what keep-last-good restores.

## Seams under test
`fmd-core` module updater tests with the stub GitHub transport and a `WorkerPool`: a sync that changes `checkantibot.lua` is used by the next request without restart; a sync that ships a broken module never lets a concurrent job load it.

## Acceptance criteria
- [ ] No restart needed after a bypass script or FlareSolverr change.
- [ ] Broken-module window closed (test fails on current main).

## FMD2 references
- `baseunits/lua/LuaWebsiteBypass.pas:101-212`
- `mangadownloader/forms/frmLuaModulesUpdater.pas:398-440`
