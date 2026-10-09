# T50: Verify the JS engine against Duktape on the untested cases
Deps: none

## Goal
T12 (PR #32) left these unverified: non-BMP characters (QuickJS surrogate pairs vs Duktape), and the Cloudflare IUAM, ReadComicOnline and remaining acqqcom snippets, which need real pages.

## Scope (in/out)
In:
- Build Duktape (the version FMD2 bundles) as a test-only reference and run the same snippets on both engines: non-BMP strings, `String.length`, `charCodeAt`, regex on astral characters, JSON round trips.
- Record real pages (via the T15/T16 recorder) for ReadComicOnline and acqqcom and add `ExecJS` fixtures from them; add a Cloudflare IUAM challenge fixture if one can be captured (or a faithful synthetic one, marked as such).
- Fix any difference that a module could observe.

Out: replacing QuickJS.

## Seams under test
Lua snippets through `fmd-lua`'s public runtime calling `fmd.duktape.ExecJS`, compared with the Duktape reference output.

## Acceptance criteria
- [ ] Every difference found is fixed or documented with why no module can observe it.

## FMD2 references
- `baseunits/Duktape.pas:77-104`, `baseunits/lua/LuaDuktape.pas:14-24`
- `lua/websitebypass/cloudflare.lua`
