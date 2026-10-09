# T70: Cover thumbnails on Discover
Deps: T71

## Goal
Discover's cards show a coloured placeholder with the title (`web/src/routes/discover/+page.svelte:205-207`, hue from `hue()` at :120). That is because a manga list has no cover: `masterlist` holds the link, title and metadata only (`crates/fmd-store/src/migrations/lists_v1.sql:5-19`), like FMD2's per-site tables (`baseunits/DBDataProcess.pas:143-153`), and FMD2-DB dumps have no cover column either. A title's cover link is only known after fetching its info page (`GetInfo` → `MangaInfo.CoverLink`), as `GET /api/series` does (`crates/fmd-server/src/series.rs:215`). Show real thumbnails where possible, keeping the coloured placeholder as the fallback.

T71 decides where covers come from: an external metadata service, the website's `GetInfo` (as below), or both. Revise this scope to follow T71's recommendation before starting.

## Scope (in/out)
In:
- **Lazy, on screen only:** a card asks for its cover when it scrolls into view (an `IntersectionObserver`, as the infinite scroll already uses). The placeholder shows until the image loads, and stays on any failure.
- **Resolving the cover:** an endpoint such as `GET /api/covers/series?module=&link=&w=` runs the module's `GetInfo` for that title, then serves the cover through the existing proxy and disk cache (`crates/fmd-server/src/covers/`), resized by `w`.
- **Remember the cover link:** store the resolved cover link for each (module, link), including "this title has no cover", so the info page is fetched once per title, not once per view. A `lists.db` migration (`crates/fmd-store/src/migrations/`). A list update or FMD2-DB import doesn't clear it. A stored link is rechecked after `covers.revalidate_after_hours`.
- **Be gentle with websites:** at most a few lookups per module at a time (respecting the module's connection limits), queued so scrolling fast doesn't fire hundreds of `GetInfo` calls. A card that scrolls away before its turn is cancelled.
- **The setting:** "Load manga covers" (`general.load_covers`) off means placeholders only, with no lookups.
- If the browser's `/api/series` load of a title already learned its cover, store that too (saves a lookup later).

Out: fetching covers during list updates or imports in bulk; covers on any page other than Discover.

## Seams under test
- `fmd-server`: the series-cover endpoint runs `GetInfo` once for a title and serves later calls from the stored link; a title with no cover returns 404 quickly without calling `GetInfo` again; the per-module concurrency cap holds under many simultaneous requests (a fake module that counts calls in flight).
- `fmd-store`: the cover-link table survives a list update and an FMD2-DB import of the same module.
- Web component test: a card shows the placeholder, then the image after it loads; a failed image keeps the placeholder; with `load_covers` off no cover request is made.
- Playwright (mocked API): scrolling Discover loads covers only for the visible cards.

## Acceptance criteria
- [ ] Discover shows cover thumbnails for titles whose site has a cover, falling back to the coloured placeholder.
- [ ] A title's info page is fetched at most once per revalidation period, and never more than a few at a time per website.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
None for the list view, which has no covers in FMD2. The cover itself comes from `MangaInfo.CoverLink` (`baseunits/lua/LuaMangaInfo.pas:27`), as on FMD2's info panel.
