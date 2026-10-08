# T16: Smoke list, recorded fixtures, CI replay and nightly live run
Deps: T15

## Goal
Pick ~30 representative upstream modules, record their HTTP traffic once, replay it in CI to catch regressions in FMD2r, and run them live nightly to catch site/module breakage separately.

## Scope (in/out)
In:
- `fixtures/smoke/list.toml`: ~30 entries `{ module_id, manga_url, chapter_url, notes }`, chosen to cover: each major template (Madara, MangaThemesia, MangaReaderOnline, FMReader, WPComics, HeanCMS, GroupLe, MangaBox, Genkan, NineManga, …), JSON-API modules, modules using `fmd.crypto`, `fmd.duktape`, `fmd.imagepuzzle`, `pb` (MangaPlus), accounts/cookies, `DynamicPageLink`, and MangaDex (used in the plan's end-to-end check).
- Recorded fixtures per entry (via T15 `--record`) for `info` and `pages`, committed (keep size reasonable: truncate image bodies, they aren't needed for info/pages).
- Snapshot tests: replay each entry and compare `MangaInfo` and page lists to committed snapshots (`insta` or plain JSON).
- CI job runs the replay suite offline.
- Nightly GitHub Actions workflow runs the list live (no replay), produces a report artifact that classifies failures as "site/module changed" (live fails, replay passes) vs "FMD2r regression" (replay fails), and does not fail the main branch status.
- A script to re-record one entry.

Out: XPath differential corpus collection (T35 hooks into these runs later).

## Seams under test
- The `fmd2r module info|pages --replay` CLI (T15) per smoke entry, compared to committed snapshots.
- The nightly report generator: given a fixture of results (live pass/fail, replay pass/fail), produces the right classification.

## Acceptance criteria
- [ ] ≥ 25 entries recorded and passing on replay in CI.
- [ ] Coverage of templates/libs listed above documented in `list.toml` notes.
- [ ] Nightly workflow exists and uploads a readable report.
- [ ] Re-record script documented.

## FMD2 references
- `lua/templates/` (template families to cover)
- `lua/modules/MangaDex.lua`, `lua/modules/MangaPlus.lua` (must be in the list)
- `docs/SUPPORTED_WEBSITES.md` (site catalogue to choose from)
- `docs/LUA-REFERENCE.md:1297-1315` (available templates)
