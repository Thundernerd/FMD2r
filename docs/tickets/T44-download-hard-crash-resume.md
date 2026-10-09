# T44: Download engine: resume after a hard crash
Deps: none

## Goal
T20's resume test drops the manager gracefully, so it never exercises a hard crash between page flushes (PR #53, "Not addressed"). Prove that a killed process resumes correctly.

## Scope (in/out)
In:
- A test that runs a download in a child process (the `fmd2r` binary or a small test binary), kills it with SIGKILL mid-chapter, restarts, and checks the task resumes, skips pages already on disk, doesn't re-run `OnGetPageNumber`, and produces a complete CBZ.
- Fix whatever the test exposes (e.g. a partially written page file counted as done, or a status left that `open` doesn't resume).

Out: changing the flush interval unless the test shows it's needed.

## Seams under test
The `fmd2r` binary (or `DownloadManager` in a child process) with a fixture module and stub site; assertions on `app.db` through `TaskRepo` and on the output files.

## Acceptance criteria
- [ ] Killed at several points (during page download, during packing), the task finishes correctly after restart.
- [ ] Partially written files are never treated as complete pages (write to temp then rename, or verify on resume).

## FMD2 references
- `baseunits/uDownloadsManager.pas` (`CheckAndActiveTaskAtStartup`, `CheckForExists`)
