# T42: Smoke list coverage gaps and module issues seen while recording
Deps: none

## Goal
Close the coverage gaps T16 recorded (PR #58) and follow up the two module issues it saw.

## Scope (in/out)
In:
- Find live sites for the templates with no smoke entry: MangaReaderOnline, FMReader, MangaBox, Genkan, NineManga, MadTheme, MangaHub. Record any that work; for ones that are genuinely dead upstream, say so in `fixtures/smoke/list.toml`.
- Node-dependent modules (`utils.nodejs`, e.g. Comix): make them replayable, e.g. record the subprocess calls through the `fmd.subprocess` `Spawner` alongside HTTP, or run node in CI against recorded HTTP. Add one such entry.
- MangaFire: `pages` relies on `MODULE.Storage` set by `GetInfo`. Check what the download engine does in a real task (does a task run `GetInfo` first in FMD2? see `uDownloadsManager.pas`), and if FMD2r differs, fix FMD2r; if FMD2 behaves the same, document it as upstream behaviour.
- MadTheme: its chapter API 301 to a new host isn't followed. Confirm whether that is the module's choice or an `fmd-http` redirect difference (`httpsendthread.pas:592-718`); fix FMD2r if it's ours, otherwise note it for upstream.

Out: fixing upstream Lua modules (report them upstream instead).

## Seams under test
`fmd-smoke run` (replay) over the new entries in CI; `fmd2r module pages` replay for MangaFire after `info` in the same run if that is the fix.

## Acceptance criteria
- [ ] Every template above is covered or documented as unavailable.
- [ ] At least one node-dependent module replays offline.
- [ ] MangaFire and MadTheme each have a recorded conclusion (fixed or upstream).

## FMD2 references
- `baseunits/uDownloadsManager.pas:1104-1330`
- `baseunits/httpsendthread.pas:592-718`
- `lua/utils/nodejs.lua`
