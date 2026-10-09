# T52: A numeric `fmd.env.Revision`
Deps: none

## Goal
Found by T42: `fmd.env.Revision` is empty unless `FMD2R_REVISION` is set at build time (`crates/fmd-lua/src/package.rs`), so a module that compares it with a number fails. `templates/MangaHub.lua:122` does `tonumber(require 'fmd.env'.Revision) < 6920` in `GetPageNumber`, which raises "attempt to compare nil with number" before any request: every MangaHub mirror's downloads fail even once its API host is reachable again. FMD2's release builds always set `REVISION_NUMBER` to the first-parent commit count of the build (`git2revision.bat`, `baseunits/lua/LuaFMD.pas:23`).

## Scope (in/out)
In:
- Give `Revision` a numeric value by default: the FMD2 revision whose Host API FMD2r reproduces (the commit count of the reference checkout), overridable at build time.

Out: anything else in `fmd.env`.

## Seams under test
A Lua snippet against the public `fmd-lua` runtime: `tonumber(require 'fmd.env'.Revision)` is a number of at least 6920.

## Acceptance criteria
- [ ] `fmd.env.Revision` is a numeric string in every build.
- [ ] MangaHub's `GetPageNumber` gets past its revision check.

## FMD2 references
- `baseunits/lua/LuaFMD.pas:23`
- `git2revision.bat`
- `lua/templates/MangaHub.lua:122`
