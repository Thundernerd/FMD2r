# T59: Download observability: task lifecycle logs and the first progress frame
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification (O4, O5).

1. At `RUST_LOG=info` the server logs nothing about downloads. Across a whole run, `/api/logs` and `docker compose logs` held only `listening`, `shutting down`, `listening`: the task start, the resume after the restart, and both finished chapters left no trace. `crates/fmd-core/src/download/` has no `info!` at all; it only warns on failures (e.g. "chapter … has no pages", `task.rs:442`).
2. The first `task.progress` frame of a new chapter carries the previous chapter's page total: `"status":"preparing","done":0,"total":26` for a 28-page chapter 2, then 28. `reset_phase` (`crates/fmd-core/src/download/task.rs:558-564`) sends a forced progress frame with `pages_total` from `task.page_number` (:200) before `get_page_number` resets it to 0 (:567-568), so at :394-396 the frame still has the last chapter's count.

## Scope (in/out)
In:
- `info` logs (target `fmd_core`) for: task started, resumed after a restart, chapter finished, task finished, task failed (with the reason), task stopped.
- A new chapter starts with a page total of 0, so its first frame doesn't show the previous chapter's total.

Out: log persistence (T48). Per-page logging.

## Seams under test
`fmd-core`'s download engine with a fixture module and stub site (as in the existing engine tests):
- A two-chapter task, chapters with different page counts: the first progress frame of chapter 2 has `pages_total` 0 or chapter 2's count, never chapter 1's.
- With a capturing `tracing` subscriber at `info`: one start and one finish line per task, one line per finished chapter, and a resume line for a task restored as downloading.

## Acceptance criteria
- [ ] A normal download, a restart mid-download and a failed task each leave `info` lines in `/api/logs`.
- [ ] No stale page total in the first frame of a chapter.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
- `baseunits/uDownloadsManager.pas:829-866` (`DoGetPageNumber`)
- `baseunits/uDownloadsManager.pas:1303`, :1355 (FMD2's task logging, warnings only)
