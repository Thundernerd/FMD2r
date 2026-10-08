# T15: Dev CLI `module init|info|pages` with HTTP record/replay
Deps: T14

## Goal
Give developers (and CI) a way to exercise one module from the command line, and capture its HTTP traffic as fixtures that can be replayed offline. This is the tool T16's smoke tests and T35's XPath corpus are built on.

## Scope (in/out)
In:
- `fmd2r module init [--lua-dir DIR] [--module ID|--file PATH]`: load modules, print a table (ID, name, root URL, callbacks, options) or JSON (`--json`); non-zero exit if any load failed, with the report.
- `fmd2r module info <url> [--module ID]`: resolve the module by host (as FMD2's `LocateModuleByHost`) unless given, run `OnGetInfo`, print the `MangaInfo` as JSON.
- `fmd2r module pages <chapter-url> [--module ID]`: run `OnTaskStart` + `OnGetPageNumber` (+ `OnGetImageURL` per page unless `DynamicPageLink`), print page links as JSON.
- `fmd2r module download <chapter-url> --out DIR`: optional stretch, may print "not implemented (T20)".
- `--record DIR`: every HTTP exchange (method, URL, request headers/body, response status/headers/body) is written to a fixture file (one JSON/CBOR file per exchange plus an index; bodies stored separately, binary-safe). `--replay DIR`: the transport serves those exchanges, matching by method+URL+body (with configurable header normalisation), and fails loudly on an unrecorded request.
- Deterministic output ordering for snapshot tests.

Out: the smoke list itself (T16).

## Seams under test
The `fmd2r` binary run as a process (`assert_cmd`) against a fixture Lua dir and a local test HTTP server:
- `module init --json` lists the fixture module.
- `module info http://127.0.0.1:PORT/manga/1 --record R` writes exchanges into `R`; then `module info … --replay R` with the server stopped prints identical JSON.
- Replay with a missing exchange exits non-zero naming the URL.

## Acceptance criteria
- [ ] All three subcommands work against fixtures; JSON output is stable.
- [ ] Record/replay round-trips binary bodies and gzip/br responses (store decoded bodies, note that in the format doc).
- [ ] Fixture format documented in `docs/fixtures.md` (or the crate README).

## FMD2 references
- `baseunits/WebsiteModules.pas:470-534` (`LocateModule`, `LocateModuleByHost`)
- `baseunits/uData.pas:85-208` (`GetInfoFromURL`: how FMD2 drives `OnGetInfo`)
- `baseunits/uDownloadsManager.pas:829-881` (`DoGetPageNumber`), `:327-333`, `:421-433` (page link resolution)
- `docs/LUA-REFERENCE.md:1777-1784` (`--lua-dofile` dev workflow in FMD2)
