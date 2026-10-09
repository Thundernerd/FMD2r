# T61: `fmd2r module pages` reports a failed `GetPageNumber`
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification. `prepare_chapter` (`crates/fmd2r/src/module.rs:330-390`) keeps only `.value.task` of the `OnTaskStart` and `GetPageNumber` replies (:341, :346) and ignores `TaskReply::ok` (`crates/fmd-lua/src/pool/callbacks.rs:169-172`). When the module returns false and leaves no page links, the empty list is padded with `UNRESOLVED_PAGE` "W" (:357, :361). So a failing chapter URL (e.g. `https://mangadex.org/chapter/<uuid>`, where MangaDex answers a 404 JSON and the module returns `net_problem`) prints `page_links: ["W"]` and exits 0, which looks like a one-page chapter.

The engine ignoring the result is FMD2-faithful (`DoGetPageNumber`, `crates/fmd-core/src/download/task.rs:566-570`); only the dev CLI's report changes.

## Scope (in/out)
In:
- `pages` output includes each callback's result (e.g. `"task_start": true, "get_page_number": false`).
- Exit non-zero when `GetPageNumber` returns false or no page link resolves, with a message on stderr; the JSON is still printed.

Out: the download engine.

## Seams under test
The `fmd2r` binary (as in its existing CLI tests) against a fixture module whose `GetPageNumber` returns false: the JSON shows `get_page_number: false` and the exit code is non-zero. A working fixture chapter still exits 0.

## Acceptance criteria
- [ ] A failing chapter URL is visible as a failure in `fmd2r module pages`.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
- `baseunits/uDownloadsManager.pas:829-881` (`DoGetPageNumber`)
