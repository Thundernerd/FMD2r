# T48: Persist logs
Deps: none

## Goal
Logs live only in memory (PR #41), so they are lost on restart, which is when they are often needed. Persist them with rotation.

## Scope (in/out)
In:
- Write log lines to `<data dir>/logs/` (JSON lines), rotated by size and count (settings with defaults, e.g. 10 MB × 5).
- On startup, load the tail into the `LogBuffer` so `/api/logs` and the System page show lines from before the restart, keeping `seq` monotonic across restarts.
- System page: "Download logs" for the persisted files.

Out: shipping logs anywhere external.

## Seams under test
`fmd-server`'s `LogBuffer` / `/api/logs` via `oneshot` across a simulated restart (new state over the same data dir); rotation unit-tested through the public writer.

## Acceptance criteria
- [ ] Lines survive a restart; `since` paging still works across it.
- [ ] Disk use bounded by the rotation settings.

## FMD2 references
- None (FMD2 logs to a file through MultiLog; see `baseunits/uBaseUnit.pas` logging calls).
