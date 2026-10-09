# T70: Cover thumbnails on Discover
Deps: T71, T73

## Goal
Discover's cards show a coloured placeholder with the title (`web/src/routes/discover/+page.svelte:205-207`, hue from `hue()` at :120). That is because a manga list has no cover: `masterlist` holds the link, title and metadata only (`crates/fmd-store/src/migrations/lists_v1.sql:5-19`), like FMD2's per-site tables (`baseunits/DBDataProcess.pas:143-153`), and FMD2-DB dumps have no cover column either. A title's cover link is only known after fetching its info page (`GetInfo` → `MangaInfo.CoverLink`), as `GET /api/series` does (`crates/fmd-server/src/series.rs:215`). Show real thumbnails where possible, keeping the coloured placeholder as the fallback.

T71 recommends taking covers from a local copy of MangaBaka's database (<https://mangabaka.org>), with the website's `GetInfo` as the fallback (`docs/research/metadata-sources.md`). T73 downloads that database and matches each list against it. In T71's probe, the match accepted 491 of 600 list titles, with 2 wrong matches. This ticket uses those stored matches for covers, and runs `GetInfo` for the titles without one, one website request per title.

## Scope (in/out)
In:
- **Lazy, on screen only:** a card asks for its cover when it scrolls into view (an `IntersectionObserver`, as the infinite scroll already uses). The placeholder shows until the image loads, and stays on any failure.
- **Resolving the cover:** an endpoint such as `GET /api/covers/series?module=&link=&w=` serves the cover through the existing proxy and disk cache (`crates/fmd-server/src/covers/`), resized by `w`. It finds the cover link in this order:
  1. The stored cover link for (module, link), if there is one (below).
  2. T73's accepted MangaBaka match for (module, link), if the database is present. Use the series' 250 px tall thumbnail (`cover.x250.x1`), or the 350 px one when `w` is over 185. It is fetched without a module's session, and the SSRF guard still applies.
  3. Otherwise, the module's `GetInfo`, as before.
- **Remember the cover link:** store the resolved cover link for each (module, link), with where it came from (`mangabaka` or `website`), including "this title has no cover". The info page is then fetched once per title, not once per view.
  - A `lists.db` migration (`crates/fmd-store/src/migrations/`).
  - A list update or FMD2-DB import doesn't clear it.
  - A stored link is rechecked after `covers.revalidate_after_hours`.
  - A database refresh that changes or removes a title's match replaces a stored `mangabaka` link.
- **Be gentle with websites:** at most a few `GetInfo` lookups per module at a time (respecting the module's connection limits), queued so scrolling fast doesn't fire hundreds of calls. A card that scrolls away before its turn is cancelled. Covers from MangaBaka's CDN go through the same disk cache, and need no `GetInfo` at all.
- **The setting:** "Load manga covers" (`general.load_covers`) off means placeholders only, with no lookups and no cover requests to MangaBaka's CDN.
- If the browser's `/api/series` load of a title already learned its cover, store that too (saves a lookup later).

Out: fetching covers during list updates or imports in bulk; downloading the database and matching (T73); per-title MangaBaka API lookups (T71 found them less accurate, and MangaBaka's terms point bulk users to the database); covers on any page other than Discover.

## Seams under test
- `fmd-server`: the series-cover endpoint runs `GetInfo` once for a title and serves later calls from the stored link; a title with no cover returns 404 quickly without calling `GetInfo` again; the per-module concurrency cap holds under many simultaneous requests (a fake module that counts calls in flight).
- `fmd-server` with a fixture `metadata.db` (T73) and a mocked cover CDN:
  - A title with an accepted match serves the MangaBaka cover without calling `GetInfo`.
  - A title with no match, or a rejected one, falls back to `GetInfo`.
  - Without the database, every title goes to `GetInfo`.
- `fmd-store`: the cover-link table survives a list update and an FMD2-DB import of the same module.
- Web component test: a card shows the placeholder, then the image after it loads; a failed image keeps the placeholder; with `load_covers` off no cover request is made.
- Playwright (mocked API): scrolling Discover loads covers only for the visible cards.

## Acceptance criteria
- [ ] Discover shows cover thumbnails from MangaBaka where T73 has an accepted match, else from the title's website, falling back to the coloured placeholder.
- [ ] A title's info page is fetched at most once per revalidation period, and never more than a few at a time per website.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
None for the list view, which has no covers in FMD2. The cover itself comes from `MangaInfo.CoverLink` (`baseunits/lua/LuaMangaInfo.pas:27`), as on FMD2's info panel.
