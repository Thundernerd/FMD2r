# T62: Series page: call the chapter mark "seen"
Deps: none

## Goal
Decided in T60 (option a). Adding a series to the library marks every current chapter in `downloaded_chapters` (`crates/fmd-server/src/favorites.rs:156-160`), as FMD2's `btAddToFavoritesClick` does (`mangadownloader/forms/frmMain.pas:2797-2846`), and imported FMD2 marks are the same mix. So the mark means "seen or downloaded", but the series page calls it "downloaded": a series just added shows every chapter as "✓ downloaded", the download box says "2 downloaded before" for chapters never downloaded, and "Hide downloaded" hides the whole list. Rename the mark to "seen" everywhere it is shown.

## Scope (in/out)
In (UI text only):
- `web/src/lib/components/series/ChapterList.svelte`: "✓ downloaded" → "✓ seen" (:130), "Hide downloaded" → "Hide seen" (:91), "Every chapter is downloaded." → "Every chapter is seen." or similar (:101).
- `web/src/lib/components/series/DownloadBox.svelte:88`: "{n} downloaded before" → "{n} seen before".
- `web/src/lib/components/series/SeriesHeader.svelte:99`: "{n} downloaded" → "{n} seen".
- Any other place the UI shows the `downloaded` chapter flag (check the library cards and the mock data's labels).

Out:
- Which chapters get marked, and the store (no new column).
- The API: `SeriesChapter.downloaded` keeps its name. Its description (`Whether the chapter was downloaded before.`) may say "downloaded, or marked when the series was added to the library".
- Download task and queue wording (`ChapterState` `downloaded`, "Chapters downloaded"): those are real downloads.

## Seams under test
- Web component tests: a chapter list with a marked chapter shows "✓ seen" and a "Hide seen" toggle that hides it; the download box counts picked marked chapters as "seen before"; the series header shows "<n> seen".

## Acceptance criteria
- [ ] The series page never calls a `downloaded_chapters` mark "downloaded".
- [ ] The web lint, check, unit tests and the Playwright tests pass.

## FMD2 references
- `mangadownloader/forms/frmMain.pas:2797-2846` (`btAddToFavoritesClick` marks the current chapters)
- `mangadownloader/forms/frmMain.pas:1955`, :3162 (hide downloaded, the mark)
