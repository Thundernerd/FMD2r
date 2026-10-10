# T86: Discover keeps its filters and place when you come back
Deps: none

## Goal
On Discover, set some filters, scroll a while, open a title, then press the browser's Back button: Discover comes back empty. Every filter is plain component state in `web/src/routes/discover/+page.svelte:46-64` (`module`, `text`/`q`, `genres`, `status`, `format`, `publication`), and so are the loaded results (`items`, `total`, `page`). None of it is in the URL or saved anywhere, so leaving the page throws it away. Back should land on the same search, the same results and the same scroll position.

## Scope (in/out)
In:
- **Filters in the URL.** The website, the search text, the included and excluded genres, the status, the format and the publication go in Discover's query string, e.g. `/discover?module=mangadex&q=one&genres_include=Action&status=1`. Reuse the API's parameter names from `searchQuery` (`web/src/lib/discover/filters.ts:50`) and leave out empty values, so a plain `/discover` is the unfiltered page. Add a parser for the other direction (`filtersFromQuery(URLSearchParams): Filters`) next to `searchQuery`. Unknown or malformed values are dropped, not errors.
- **Changing a filter replaces the history entry** (`goto(url, { replaceState: true, noScroll: true, keepFocus: true })`), so Back from Discover still leaves Discover instead of stepping through each filter change. The search text writes the URL once the debounce settles (`DEBOUNCE_MS`, :19), not on every key, and typing keeps focus in the box.
- **Opening Discover from a URL applies its filters.** A reload, a pasted link or Back all start from the query string. The search box shows the text from `q`.
- **Results and scroll come back on Back.** Use a SvelteKit snapshot (`export const snapshot` in the route) to capture `items`, `total` and `page`, plus the search text. Restoring them renders the grid at once, so SvelteKit's own scroll restoration finds the page tall enough and lands where you left. The first search after restoring must not wipe the restored items with a fresh page 1 when the filters haven't changed. If no snapshot exists (a reload or a new tab), load page 1 as today.
- A filter change made while an older search is in flight still wins (the `generation` check, :108-122, stays).

Out:
- The filter drawer's open/closed state on phones; it starts closed.
- Remembering filters across visits that don't go through history (opening Discover from the nav bar gives a clean page).
- The series page's "← Library" link (`web/src/routes/series/+page.svelte:86`), which always goes to Library even when you came from Discover. That's a separate fix.
- Server changes: the search API already takes all these parameters.

## Seams under test
- Unit (`web/src/lib/discover/filters.test.ts`): `filtersFromQuery(new URLSearchParams(searchQuery(f)))` gives back `f` for filters with every field set, and for empty filters; bad values (an unknown status, an empty genre name) are dropped.
- Route test (`web/src/routes/discover/page.test.ts`): opening the page with `?module=…&genres_include=…&status=…` sends that search and shows those filters selected; changing a filter calls `goto` with the new query and `replaceState: true`.
- Playwright (`web/e2e/discover.test.ts`):
  - pick a website, type a search, include a genre and pick a status; scroll until a second page has loaded; open a title; `page.goBack()`. The filters, the search box, the title count and the number of cards are as before, and `window.scrollY` is within a card's height of where it was;
  - load `/discover?q=…&status=…` directly and the results match those filters;
  - change three filters, then `page.goBack()` once: it leaves Discover.

## Acceptance criteria
- [ ] Back from a series page returns to Discover with the same filters, results and scroll position.
- [ ] A Discover URL with filters can be reloaded or shared and opens the same search.
- [ ] Filter changes don't add history entries.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None. FMD2 is a desktop app whose filter panel simply stays as it was while you look at a title.
