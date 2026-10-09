# T71: Research external metadata sources for list titles
Deps: none

## Goal
Manga lists hold only a link, title and some text metadata per title (`crates/fmd-store/src/migrations/lists_v1.sql:5-19`), with no cover. T70 plans to get covers by running each title's `GetInfo` on the source website. That costs one request to the website per title, and many sites have small or broken cover images. A metadata service such as MangaBaka or AniList could provide covers (and possibly better descriptions, genres, status and alternative titles) without loading the source websites. This ticket finds out whether that works well enough, and decides how T70 gets its covers.

This is a research ticket: the output is a written recommendation and a measured probe, not production code.

## Scope (in/out)
In:
- **Survey the candidates:** MangaBaka, AniList, MangaDex, MangaUpdates, Kitsu, MyAnimeList (via Jikan). For each, note:
  - API shape and auth: whether a key is needed, and whether each install would need its own.
  - Rate limits. AniList documents 90 requests/minute, currently degraded to 30 (<https://docs.anilist.co/guide/rate-limiting>).
  - Terms of use, especially caching. AniList forbids using the API as data storage and "hoarding or mass collection" (<https://docs.anilist.co/guide/terms-of-use>), which matters for caching covers of whole lists.
  - Whether bulk dumps exist.
  - Cover image sizes and hotlinking rules.
  - Which metadata fields are available.
  - Coverage of the kinds of titles FMD2's websites list (manga, manhwa, manhua, webtoons, scanlation-only titles).
- **What is already known about MangaBaka** (<https://mangabaka.org>), to be confirmed:
  - **API:** a JSON API at `https://api.mangabaka.org/` (docs at <https://mangabaka.org/data/api>, explorer at `/data/api/explorer`). Rate limits are per IP: 30 searches/minute and 180 other requests/minute. Cached responses (`cf-cache-status: HIT`) don't count.
  - **Dumps:** nightly database dumps in JSON, JSONL and SQLite, compressed as tar.gz or zst (<https://mangabaka.org/data/database>, e.g. `https://api.mangabaka.org/v1/database/series.json.tar.gz`), with cross-site IDs for AniList, Anime-Planet, Kitsu, MangaUpdates, MyAnimeList and Shikimori.
  - **To check:** whether covers are in the dump. A local dump could be matched offline, like FMD2-DB, with no per-title requests.
  - **Licence:** attribution is required. The API page says CC BY-NC-SA 4.0 for MangaBaka's own data and the database page says CC BY-SA 4.0; resolve which applies. Third-party fields keep their providers' terms. Read the terms and acceptable use policy at `/about`.
- **Matching:** how a list entry (module, title, alt titles, authors) would be matched to an external entry, given that list titles are often romanized, translated or decorated (e.g. "(Official)", "[Colored]").
  - Use the cross-links services publish. MangaBaka aggregates IDs from other trackers; some source websites are trackers themselves, e.g. a MangaDex module's links are MangaDex IDs.
  - Set a confidence threshold below which the cover is not shown.
- **Probe:** a throwaway script (kept under `docs/research/` or `scripts/`, not in the crates) that matches a sample of about 200 titles from 3–4 real lists against the top one or two candidates. Report the match rate, wrong-match rate (checked by hand on a subset), and requests used.
- **Recommendation**, written to `docs/research/metadata-sources.md`. Pick one:
  - (a) an external source for covers, with `GetInfo` as the fallback;
  - (b) `GetInfo` only, as T70 is written now;
  - (c) external first, for metadata beyond covers too.

  Include the user-visible settings it needs (e.g. a source picker, an off switch, an API key) and privacy notes: title searches leave the install for a third party.
- Update T70's scope to match the recommendation, and draft a follow-up ticket if metadata beyond covers is worth it (e.g. richer Discover filters or series-page details).

Out: production code, UI, schema changes; tracking or syncing reading progress to these services (a separate idea).

## Seams under test
None (research). The probe's numbers and how to rerun it go in the write-up.

## Acceptance criteria
- [ ] `docs/research/metadata-sources.md` compares the candidates on the points above, with links to their docs and terms.
- [ ] The probe's match and wrong-match rates are reported for the leading candidates.
- [ ] A recommendation is made, and T70 is updated to follow it (or left as is, with the reason).

## FMD2 references
None (FMD2 uses only the source websites' own metadata).
