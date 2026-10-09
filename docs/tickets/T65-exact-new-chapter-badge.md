# T65: An exact new-chapter badge
Deps: none

## Goal
Decided in T51 (item 3; the "seen" display is T60/T62). The library's new-chapter badge is `current_chapter − rows in downloaded_chapters` (`crates/fmd-server/src/favorites.rs:362`): two counts, never compared by link, as its own comment says (:38-40). Marks the site no longer lists (removed chapters, a changed URL scheme, old FMD2 imports) make it wrong in both directions. E.g. a site drops 5 chapters and adds 3: 48 on the site, 50 marks, the badge shows 0. The favorites checker itself compares by link (`crates/fmd-core/src/favorites.rs:732-749`).

## Scope (in/out)
In:
- The checker stores the site's chapter links for each favorite at each check (a store migration, `crates/fmd-store/src/migrations/`).
- The badge is the number of stored links with no mark in `downloaded_chapters` (case-insensitive, as the table's `NOCASE` key).
- A favorite never checked since the upgrade (or imported from FMD2) keeps today's count until its first check.

Out: how chapters get marked, and the "seen" wording (T62).

## Seams under test
- `fmd-server`: `GET /api/favorites` after a check where the site dropped 5 marked chapters and added 3 reports `new_chapters: 3`; with stale marks from an old URL scheme, it counts the current links that are unmarked.
- `fmd-core`: a check stores the site's link list, replacing the previous one.

## Acceptance criteria
- [x] The badge equals the number of chapters the site lists that are not marked.
- [x] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
None (this goes beyond FMD2, which stores only the `currentchapter` count).
