# T87: Group Discover's filters by where they come from
Deps: none

## Goal
Discover's filter panel puts Status, Format, Publication and the genre chips in one flat column (`web/src/routes/discover/+page.svelte:216-246`). Status and Publication offer almost the same choices (Completed, Ongoing, Hiatus, Cancelled), and nothing says why both are there. They come from different sources:
- **Status** and **Genres** come from the website's own list, as FMD2 stores it (`MangaInfo_Status*`, `baseunits/uBaseUnit.pas:230-233`; genres via `crates/fmd-core/src/lists/info.rs:33`).
- **Format** and **Publication** come from the MangaBaka database matched to list titles (T73), and only show once it is downloaded.

Split the panel into two labelled groups, website filters and metadata filters, so the source of each filter is plain.

## Scope (in/out)
In:
- The website picker and the list actions (`WebsitePicker`, `ListActions`) stay at the top. They choose what gets searched and aren't filters on title data.
- **Website filters** group: Status, then the genre chips. A short caption says where the data comes from: "From each website's list."
- **Metadata filters** group: Format, then Publication. Caption: "From MangaBaka." The group sits below the website filters.
- Each group is a `<fieldset>` with a `<legend>` (or `role="group"` with `aria-labelledby`), so assistive tech, and the tests, can find a filter within its group.
- When the MangaBaka database is available but not downloaded, the metadata group shows a muted line in place of its filters: "Download the MangaBaka database to filter by format and publication." It links to `/settings#section-metadata`, like `MangaBakaHint`. This line stays even after the hint at the top of the page has been dismissed. When MangaBaka isn't available at all (`status.available` false), the group is left out.
- The labels Status, Format and Publication stay the same; the group headings make the difference clear.
- The phone drawer (`.facets.open`, :392-) shows the same groups.

Out:
- Merging Status and Publication into one filter, or changing which value the card subtitle shows (`subtitle()`, :165).
- Server changes: the search and facet APIs stay the same.
- The filters in the URL (T86): the query parameter names don't change.

## Seams under test
- Route test (`web/src/routes/discover/page.test.ts`, `facets.test.ts`): the Status combobox and the genre chips are inside the "Website filters" group; Format and Publication are inside the "Metadata filters" group once MangaBaka is downloaded; when it's available but not downloaded, that group holds the download line and link and no comboboxes; when it's unavailable, there's no metadata group.
- Playwright (`web/e2e/discover.test.ts`): update the MangaBaka test (:135-) to find Format and Publication within the metadata group, before and after the download.

## Acceptance criteria
- [ ] Discover's filters are split into "Website filters" (Status, Genres) and "Metadata filters" (Format, Publication), each saying where its data comes from.
- [ ] Without the MangaBaka database, the metadata group says how to get it.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
FMD2 has no MangaBaka metadata. Its Filter tab (`tsinfoFilterAdv`, `mangadownloader/forms/frmMain.lfm:1439`, with `cbFilterStatus` at :1613) only has the website list's own fields, which here become the website filters.
