# T68: A clear message when FMD2-DB has no list for a website
Deps: none

## Goal
"Get the list from FMD2-DB" (`web/src/lib/components/discover/ListActions.svelte:107`) downloads `<module id>.7z` from `update_lists.db_url` (`crates/fmd-core/src/settings/model.rs:393-396`). FMD2-DB has no dump for many modules, and the user then sees the raw error, e.g. `downloading https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/ba5c1a22af434aaca6c8c6874b7f54ec.7z failed with HTTP status 404` (`ImportError::Download`, `crates/fmd-core/src/lists/import.rs:23-24`). It reaches the UI as a plain string (`ListEvent.error`, `crates/fmd-core/src/lists/jobs.rs:54-55`, prefixed with the module ID at :270), shown in Discover (`ListActions.svelte:123-124`) and in System → Jobs as "Last error" (`web/src/lib/components/system/JobsPanel.svelte:112-116`). It names an opaque URL and hash, and doesn't say what to do.

## Scope (in/out)
In:
- A 404 from the dump URL is its own outcome: FMD2-DB has no list for this website. The UI says so in words, naming the website, e.g. "FMD2-DB has no ready-made list for MangaDex. Use Update list to build it from the website.", with the "Update list" action next to it.
- Other failures get a short, readable message too: the website or GitHub could not be reached (connection errors, 5xx), the download was cancelled, the archive was damaged or empty. The details (URL, status, underlying error) stay available: in the log, and behind a "Details" disclosure in the UI.
- The list event carries a machine-readable reason next to `error` (e.g. `reason: "no_dump" | "unreachable" | "bad_archive" | …`), so the UI picks the message from the reason, not by parsing text. Update the OpenAPI spec (`openapi.json`) and the web types.
- The Jobs panel's "Last error" shows the same readable text.
- The import still leaves the module's list as it was.

Out: checking which modules have a dump before the user clicks (e.g. a HEAD request or an index of FMD2-DB); changing the default `db_url`.

## Seams under test
- `fmd-core`: an import whose dump URL answers 404 finishes with the `no_dump` reason; a 500 or a connection error gives `unreachable`; a corrupt archive gives `bad_archive`. The list is unchanged in each case.
- `fmd-server`: the `job.lists.finished` event for a failed import includes the reason.
- Web component test (`ListActions`): a `no_dump` failure shows the website name and an "Update list" button that starts an update; the raw URL is only inside "Details".

## Acceptance criteria
- [ ] A missing FMD2-DB dump shows a plain message naming the website and pointing to "Update list", not a URL and HTTP status.
- [ ] Other import failures read clearly, with the technical details still reachable.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks and tests pass.

## FMD2 references
`TDBUpdaterThread` (`baseunits/DBUpdater.pas:125`) treats any result code of 300 or more as a failed download.
