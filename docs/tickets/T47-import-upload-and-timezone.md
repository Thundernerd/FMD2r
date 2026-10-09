# T47: Import from the web UI, and FMD2's local timestamps
Deps: none

## Goal
The importer (T32, PR #40) is CLI-only and imports FMD2's zone-less local timestamps as UTC. Add the optional upload endpoint and a time-zone option.

## Scope (in/out)
In:
- `POST /api/import` taking a zip of an FMD2 `userdata` folder (size-limited, extracted to a temp dir), with `dry_run`, path maps and `resume` options; returns the `ImportReport`. Runs as a background job with progress.
- Library page "Import from FMD2" (in the prototype) uploads, shows the dry-run report, then imports.
- `--timezone`/`timezone` option (IANA name, default the server's local zone) for interpreting FMD2's timestamps.

Out: importing from a live FMD2 instance.

## Seams under test
HTTP handler via `oneshot` with a zipped fixture userdata dir (T32's fixtures): dry run writes nothing; import matches the CLI result; timestamps convert with the given zone. Playwright (mock) for the upload flow.

## Acceptance criteria
- [ ] Zip-slip and oversized uploads refused.
- [ ] Same report as `fmd2r import` for the same input.

## FMD2 references
- `baseunits/DownloadsDB.pas`, `baseunits/FavoritesDB.pas` (DATETIME columns)
