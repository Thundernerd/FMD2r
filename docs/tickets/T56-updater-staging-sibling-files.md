# T56: Module updater: validate staged modules with their sibling files
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification (O1). On a fresh start with the bundled snapshot, the inbox showed:
```
module MangaPlus.lua failed Init
/data/lua/modules/MangaPlus.lua:99: /data/lua/.fmd2r-staging/modules/MangaPlus.proto: No such file or directory
```
With keep-last-good, the updater downloads a changed module file whose current version is loaded into `.fmd2r-staging` (`to_stage`, `crates/fmd-core/src/module_updater.rs:822-836`) and loads it from there to validate it (`validate`, :841-861). MangaPlus's top-level chunk reads `MangaPlus.proto` from its own directory (`debug.getinfo(1, 'S').source`, `lua/modules/MangaPlus.lua:98-110`), but only module `.lua` files are staged, so the `.proto` isn't there and validation fails. The error is a false alarm about a file that works, and the new MangaPlus.lua is never applied: the live tree keeps the old version.

FMD2 has no staging step; its updater writes the files into the Lua dir and `DoInit` loads them from there (`baseunits/lua/LuaWebsiteModules.pas:473-500`).

## Scope (in/out)
In:
- Make a staged module see the files next to it as it would in the Lua dir: either stage the module's non-module siblings too (new versions where this sync changed them, the live ones otherwise), or validate with the live directory's files on the path the module resolves.
- The validation error, if any, still names the live file (as `validate` does now).

Out: hot reload of the anti-bot scripts and the broken-module window (T46).

## Seams under test
`fmd-core`'s module updater against a stub GitHub API (as in its existing tests), with a fixture module that reads a sibling data file at load time:
- A sync that changes the module but not the sibling validates it, applies it, and posts no "failed Init" item.
- A sync that changes both applies both.
- A module that really fails `Init` is still kept back with an inbox error.

## Acceptance criteria
- [ ] No false "failed Init" for MangaPlus.lua (or any module reading a sibling file) on a fresh start.
- [ ] A changed module that reads a sibling file is applied.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
- `lua/modules/MangaPlus.lua:98-110`
- `baseunits/lua/LuaWebsiteModules.pas:473-500` (`DoInit`)
- `mangadownloader/forms/frmLuaModulesUpdater.pas` (writes into the Lua dir)
