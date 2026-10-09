# T73: MangaBaka database for list metadata
Deps: T71

## Goal
Manga lists hold a link, a title and some text metadata per title, with no cover and no format (`crates/fmd-store/src/migrations/lists_v1.sql:5-19`). T71 chose MangaBaka's database as the source for covers and richer metadata (`docs/research/metadata-sources.md`). MangaBaka publishes nightly dumps with, for each series:

- titles in many languages, authors and artists;
- format (manga / manhwa / manhua / OEL), publication status, genres and tags, year, content rating, description;
- cover thumbnails;
- links to official sites, and IDs on AniList, MangaUpdates, MAL, Kitsu and others.

In T71's probe, matching four real lists against the dump offline accepted 491 of 600 titles. Checking by hand found 2 wrong matches, and no title left the install. Add an opt-in local copy of the database, match each list against it, and use the metadata on Discover and the series page. T70 then uses the matches for covers.

## Scope (in/out)
In:
- **Download and build:** a background job, reported like list jobs, that downloads `https://api.mangabaka.org/v1/database/series.jsonl.zst` (~390 MB) and builds a compact `metadata.db`.
  - Keep only what matching and display need: the series ID; normalised titles; people; type; status; genres; year; content rating; description; cover thumbnail URLs; links whose site IDs are stable (WebToons `title_no`, for a start); cross-site IDs.
  - Map each merged series' titles and IDs to the series it was merged into, and skip deleted ones.
  - Stream the download and decompression rather than holding the file in memory.
  - Swap the new file in atomically. A failed or cancelled build leaves the old one in place.
  - Tolerate fields MangaBaka adds. If a field the matcher needs is missing, the build fails with a clear message.
- **Settings → "MangaBaka database":**
  - Download / Update / Remove actions, the database's date and size, and job progress.
  - Automatic refresh every `metadata.mangabaka.refresh_days` days (default 7; 0 = off).
  - Opt-in: nothing is downloaded until the user asks.
  - Until then, Discover shows one dismissible hint pointing to the setting.
- **Matching** (T71's "Matching" rules, `docs/research/metadata-sources.md`). It runs after a list update or FMD2-DB import, for the entries that are new or whose title changed, and for every entry after a database refresh, since MangaBaka's titles and IDs change too. It tries, in order:
  1. **Site link:** WebToons `title_no`, compared with the series' links.
  2. **Cross-site IDs:** for the MangaDex module, batch `GET https://api.mangadex.org/manga?ids[]=…` (100 per request, an honest User-Agent, under ~5 requests/s). Look up `links.al/mu/mal/kt/ap` against the database's cross-site IDs.
  3. **Title, then authors:**
     - Normalised titles and alt titles, with and without bracketed decorations. Alt titles are split on the list's own separator.
     - Novel entries are dropped.
     - When the list names people, at least one must match the series' people, allowing for romanisation differences and swapped syllables. Otherwise the match is rejected.
     - Ties are broken by the main title. A title matching more than 50 series is ambiguous.

  Store one row per (module, link): the series ID and the confidence (`link`, `cross-id`, `title+author`, `title-unique`, or a rejected one). Only the accepted confidences are used anywhere.
- **Discover:**
  - Format facet (manga / manhwa / manhua / OEL / other) and status facet (ongoing / completed / hiatus / cancelled), next to the list's own genres.
  - Titles without an accepted match stay listed, with the value "unknown".
  - The search API takes the new filters. Update `openapi.json` and the web types.
- **Series page:** when the website's `GetInfo` gives no summary, show MangaBaka's description, marked "from MangaBaka". Show the format and year when known.
- **Attribution and licence:**
  - About → Credits names MangaBaka, links its data licence (CC BY-NC-SA 4.0, <https://mangabaka.org/about/data-license>), and says that fields from AniList, MAL, MangaUpdates and others keep their providers' terms.
  - Never ship the dump or `metadata.db` in a release or Docker image. Each install downloads its own.

Out:
- Covers on Discover (T70, which uses this ticket's matches).
- Per-title MangaBaka API lookups.
- A "wrong match" correction UI, and accepting `author-conflict` matches through MangaBaka's adaptation relationships (later, if wrong or missing matches show up in use).
- Syncing reading progress or libraries to MangaBaka or any tracker.
- Other sources (AniList, MangaUpdates, …).

## Seams under test
- `fmd-core`, building `metadata.db` from a small recorded JSONL fixture:
  - It holds active, merged and deleted series correctly.
  - A field the matcher needs, if missing, fails the build and leaves the old database in place.
- `fmd-core`, matching fixture list entries (cases taken from T71's probe, `docs/research/metadata-probe/results-*.csv`):
  - A WebToons link match.
  - A MangaDex cross-ID match, against a mocked MangaDex API.
  - Title + author with a romanisation difference.
  - Decorations stripped ("(Colored)").
  - An author conflict rejected.
  - A novel candidate dropped.
  - An ambiguous title rejected.
  - A list update re-matches only new or retitled entries; a database refresh re-matches all of them.
- `fmd-server`: the Discover search filters by format and status; the database job's start, progress and finish events.
- Web component tests: the Settings database panel (download, date and size, remove), the Discover hint, and the format and status facets.

## Acceptance criteria
- [ ] With the database downloaded, list titles carry MangaBaka matches, and Discover filters by format and status, with no per-title requests to MangaBaka.
- [ ] Without it, nothing is downloaded and Discover works as before.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
None (FMD2 uses only the websites' own metadata).
