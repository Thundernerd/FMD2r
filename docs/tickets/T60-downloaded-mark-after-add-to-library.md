# T60: Decide how the series page shows chapters marked when added to the library
Deps: none
Owner: user

## Goal
Found by the 2026-10-09 end-to-end verification (O3). Adding a series to the library marks every current chapter in `downloaded_chapters` (`crates/fmd-server/src/favorites.rs:156-160`), as FMD2's `btAddToFavoritesClick` does (`mangadownloader/forms/frmMain.pas:2797-2846`), so the new-chapter check only reports chapters published later. The series page can't tell those apart from real downloads: every row shows "✓ downloaded" (`web/src/lib/components/series/ChapterList.svelte:130`), the download box says "2 downloaded before" for chapters never downloaded (`DownloadBox.svelte:88`), and "Hide downloaded" (`ChapterList.svelte:91`) hides the whole list. FMD2 draws them the same way (`frmMain.pas:3162`, hide at :1955).

Keep the data as it is (FMD2 parity, and the importer brings FMD2's marks over unlabelled). The decision is how the UI should present it:
- (a) Recommended: rename the mark everywhere, since it means "seen or downloaded" in FMD2 too: "✓ seen" (or "✓ have"), "2 seen before", "Hide seen". UI-only.
- (b) Record why a chapter was marked (downloaded vs added to the library) and show "✓ downloaded" only for real downloads. Needs a store column; imported FMD2 marks stay unlabelled.

## Scope (in/out)
In: the decision; then a follow-up ticket for the chosen option.

Out: changing which chapters get marked.

## Seams under test
None (decision).

## Acceptance criteria
- [x] Option chosen and recorded in `docs/plan.md`.
- [x] A ticket for the chosen option, if it needs code.

## Decision (2026-10-09)
Option (a): the mark means "seen or downloaded", as in FMD2, and the UI calls it "seen". The data stays as it is. The rename is T62.

## FMD2 references
- `mangadownloader/forms/frmMain.pas:2797-2846` (`btAddToFavoritesClick`)
- `mangadownloader/forms/frmMain.pas:1955`, :3162 (hide downloaded, the downloaded mark)
