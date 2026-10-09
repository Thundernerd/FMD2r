# T67: A loading skeleton for the series page
Deps: none

## Goal
While a series loads, its page shows only "Loading series…" (`web/src/routes/series/+page.svelte:76-77`), then the header, chapter list and download box pop in. Fetching from the website can take seconds. Show a skeleton of the page instead: placeholder shapes laid out like the loaded page, so it doesn't jump when the data arrives.

## Scope (in/out)
In:
- A `SeriesSkeleton` component (`web/src/lib/components/series/`) with the page's layout: a header block (cover, title, a few metadata lines, buttons, as `SeriesHeader.svelte`), a chapter list of placeholder rows in `.list`, and the download box in `.side`, using the same `.body` flex layout so the sizes match.
- Placeholders use theme tokens so they work in light and dark mode, with a subtle shimmer that is off under `prefers-reduced-motion`.
- Accessibility: the skeleton is `aria-busy="true"` with a visually hidden "Loading series…" in the existing `aria-live` region; the placeholder shapes are `aria-hidden`.
- The "← Library" link stays usable while loading. Errors still replace the skeleton with the existing message.

Out: skeletons on other pages; caching or speeding up the fetch.

## Seams under test
- Web component test: while `getSeries` is pending, the page renders the skeleton (busy, with "Loading series…" for screen readers) and no chapter list; once it resolves, the skeleton is gone and the header shows the title; on a 404 the error message replaces it.
- Playwright: a slow mocked `/api/series` shows the skeleton, then the series (e.g. in `web/e2e/library.test.ts`).

## Acceptance criteria
- [ ] Loading a series shows a skeleton of the page, not a text message.
- [ ] The skeleton respects reduced motion and both themes.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None (FMD2 is a desktop app and blocks on the fetch).
