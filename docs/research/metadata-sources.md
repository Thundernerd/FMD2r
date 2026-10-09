# External metadata sources for list titles (T71)

Checked on 2026-10-09. Rate limits, terms and coverage change, so recheck the linked pages before relying on a number here.

## Question

A manga list row holds a link, a title and some text metadata, but no cover (`crates/fmd-store/src/migrations/lists_v1.sql:5-19`). T70 planned to get each cover by running the module's `GetInfo`: one request to the website per title, and often a small or broken image. Can a metadata service supply covers instead, and maybe better metadata too, without loading the websites?

## Recommendation

**(c) External first, for metadata beyond covers too, through a local copy of MangaBaka's database. The website's `GetInfo` stays the fallback for covers.**

The maintainer confirmed this direction while T71 was in review: metadata goes through the MangaBaka database.

- **Best match rate in the probe.** Matching offline against MangaBaka's nightly dump accepted **491 of 600** sampled list titles (82%) across four real lists. Checking by hand found 2 wrong matches. Both came from FMD2-DB entries whose alt titles belong to a different work.
  - MangaBaka's API, searching by title, accepted 460 (77%).
  - AniList accepted 232 of 400 (58%).
  - Details are in the [probe](#probe).
- **The dump allows matching the API can't do.** Site links match WebToons titles by `title_no` (101 of 150 WebToons titles). Cross-site IDs match MangaDex titles through the AniList, MangaUpdates, MAL and Kitsu IDs on their MangaDex entries (101 of 150).
- **No titles leave the install.** The only request to MangaBaka is the dump download, plus the cover images themselves (below).
- **No API key, nothing per install, and within MangaBaka's terms.** Its data licence asks bulk users to use the dump rather than the API ("If you need a full or substantial dataset, use the database download", [§6.2](https://mangabaka.org/about/data-license)). Matching a whole list through the API would be the bulk harvesting it forbids.
- **Covers come pre-resized.** 150, 250 and 350 px tall, plus @2x and @3x, from a CDN with `access-control-allow-origin: *` and a one-year cache. 98% of its comic entries have one.
- **Metadata the lists lack.** Format (manga / manhwa / manhua / OEL), publication status, genres and tags, year, content rating and descriptions. These feed Discover's facets and fill gaps on the series page.
- **Unmatched titles keep T70's original path.** About 1 in 5 titles go unmatched, often because the list's authors disagree with MangaBaka's. For those, the cover still comes from `GetInfo`. No title ends up worse off than under (b).

The costs:

- A ~390 MB download (`series.jsonl.zst`), refreshed weekly at most (MangaBaka rebuilds it nightly).
- A local index. The probe's unoptimised one is 314 MB and builds in about a minute.
- The dump schema "is subject to change" (no 1.0 yet), so the importer must tolerate added fields and fail loudly on removed ones.

Not (a), the per-title API lookup: fewer matches than the dump, every title on screen goes to a third party, and whole lists can't be matched without breaking the no-bulk rule.

Not (b), `GetInfo` only: one website request per title on screen, with each site's own small or broken covers.

**Tickets:**

- [T73](../tickets/T73-mangabaka-metadata.md) (new): the database download, offline matching, and the metadata on Discover and the series page.
- T70 (`docs/tickets/T70-discover-cover-thumbnails.md`): updated to take covers from T73's stored matches first, with `GetInfo` as the fallback. It now depends on T73.

### What the user sees

- **Settings → "MangaBaka database"**, opt-in, because of the download size:
  - Download / Update / Remove actions.
  - The database's date and size.
  - Automatic refresh (`metadata.mangabaka.refresh_days`, default 7, 0 = off).
- Until a database is downloaded, Discover shows one dismissible hint: "Get covers, formats and status for list titles from MangaBaka (~390 MB download)."
- **"Load manga covers"** (`general.load_covers`, already exists) stays the off switch for cover images, whatever their source.
- **No API key.** MangaBaka, AniList, MangaDex, MangaUpdates and Kitsu all serve public data without one. Only the official MyAnimeList API would need a per-install client ID.
- **Attribution.** About → Credits names MangaBaka, links its data licence (CC BY-NC-SA 4.0), and notes that third-party fields keep their providers' terms. MangaBaka asks for attribution in an About page or README ([API page](https://mangabaka.org/data/api), "Data Attribution").

### Privacy

- Matching is local. No title, list or library leaves the install.
- The dump download carries no user data. It comes from MangaBaka's Cloudflare-fronted API, which sees the server's IP and that it downloaded the dump.
- **Cover images are still fetched from `cdn.mangabaka.dev`** (through FMD2r's cover proxy and disk cache), so MangaBaka's CDN sees which series' covers the server loads. That is less than a title search reveals, and each cover is fetched once per cache period.
  - An install that wants nothing sent to MangaBaka beyond the dump can turn off "Load manga covers", or not download the database.
- For the MangaDex module, the cross-ID route reads the MangaDex entries' links from MangaDex's API: 100 per request, during matching after a list update. That is the same site the list came from.
- Without the database, everything stays between FMD2r and the manga websites, as in FMD2.

## Candidates

Primary sources were read on 2026-10-09. A "live" figure was measured with a request that day.

| | **MangaBaka** | **AniList** | **MangaDex** | **MangaUpdates** | **Kitsu** | **MAL (Jikan / official)** |
|---|---|---|---|---|---|---|
| API | REST/JSON, [docs](https://mangabaka.org/data/api), [OpenAPI](https://mangabaka.org/api.json) | GraphQL, [docs](https://docs.anilist.co) | REST, [docs](https://api.mangadex.org/docs/) | REST, [docs](https://api.mangaupdates.com/) | JSON:API, [docs](https://kitsu.docs.apiary.io/) | Jikan REST, [docs](https://docs.api.jikan.moe/); official v2, [docs](https://myanimelist.net/apiconfig/references/api/v2) |
| Key for public reads | none | none | none (an honest User-Agent is required) | none | none | Jikan: none. Official: a client ID per app that "you may not share" ([agreement](https://myanimelist.net/static/apiagreement.html)), so one per install |
| Rate limit | 30 searches/min, 180 other requests/min per IP; CDN hits don't count ([API page](https://mangabaka.org/data/api)) | 90/min, **degraded to 30/min** (live `x-ratelimit-limit: 30`); raises not accepted ([rate limiting](https://docs.anilist.co/guide/rate-limiting)) | ~5/s per IP ([limitations](https://api.mangadex.org/docs/2-limitations/)) | not published; "reasonable spacing" and caching required | not published | Jikan 60/min and 3/s; official not published |
| Caching / bulk terms | "Reasonable caching for performance purposes is permitted"; "no bulk harvesting via API", use the dump instead ([data licence §6.2](https://mangabaka.org/about/data-license)) | "Hoarding or mass collection of data … strictly prohibited"; no use as "backup or data storage"; no use in "competing … list or tracker services" ([terms](https://docs.anilist.co/guide/terms-of-use)) | no caching limits; "MUST credit MangaDex", no ads or paid services ([AUP](https://api.mangadex.org/docs/)) | "employ caching mechanisms"; must credit MangaUpdates | none published (the ToS page could not be read) | MAL ToS: no scraping, and "not to collate or aggregate any of the content" ([ToS](https://myanimelist.net/about/terms_of_use)); Jikan's terms link is dead |
| Bulk dump | **yes**: nightly JSON, JSONL and SQLite, tar.gz or zst (~390 MB), with cover URLs and cross-site IDs ([database](https://mangabaka.org/data/database)) | no | no | no | no | no |
| Cover sizes | thumbnails 150, 250 and 350 px tall (×1/×2/×3) from `cdn.mangabaka.dev`, plus the original (on average 356 px wide) | 100×150, 230×345, 460×690 (`s4.anilist.co`) | 256 and 512 px wide, plus original | thumb ~106×150, original ~283×400 | 110×156 up to 550×780, plus original | small, normal, large |
| Hotlinking | CDN sends `access-control-allow-origin: *` and a one-year cache | no rule published | "We will serve the wrong response for any image hotlinked … you MUST proxy" | none published | none published | none published |
| Fields | titles in many languages, native and romanized; description; type (manga / manhwa / manhua / OEL / novel); status; genres and tags; authors and artists; year; content rating; publishers; links to official sites; IDs on AniList, Anime-Planet, Kitsu, MangaUpdates, MAL, Shikimori, ANN | titles, synonyms, description, genres, rich tags, status, format, country of origin, staff, `idMal` | titles and alt titles, descriptions, tags, status, original language, year, content rating, authors, `links` (AniList, MU, MAL, Kitsu, AP, NU) | titles, description, type, genres, categories, status (free text), authors, publishers | titles, synopsis, subtype (manga / manhwa / manhua), status, categories, `mappings` (AniList, MAL, MU) | titles, synopsis, type, status, authors, genres and themes |
| Coverage of FMD2's kinds of titles | measured: 82% of 600 list titles (MangaDex 74%, MangaFire 88%, Asura Scans manhwa 72%, WebToons 93%); covers doujinshi and scanlated manhwa/manhua | measured: 58% of 400 (MangaDex 36%, MangaFire 67%, Asura Scans 65%, WebToons 64%); misses doujinshi and most WebToons originals | not probed; scanlation-driven, strong on manhwa/manhua, includes doujinshi | not probed; reputedly the broadest for scanlated and obscure titles | not probed; smallest catalogue, includes manhwa/manhua subtypes | not probed; weak on manhua (Martial Peak has no MAL ID on AniList) |
| Size (comics) | 306k active series, of which 277k are comics: 205k manga, 22k manhwa, 12k manhua, 37k other, 1.5k OEL; 98% have a cover | 141k manga entries | 115k | not countable (search stops at 10,000), believed largest for scanlated titles | 63k | MAL catalogue |
| Search | `/v1/series/match` (exact full title, any language, edge-cached 7 days) and `/v1/series/search` (fuzzy) | fuzzy, includes synonyms | fuzzy over alt titles | fuzzy over associated titles | full text over titles | fuzzy |

### MangaBaka in more detail

- **Licence: CC BY-NC-SA 4.0.** The ticket noted that the database page said CC BY-SA 4.0. Today both pages, and the [data licence](https://mangabaka.org/about/data-license) (last updated 2026-03-24), say **CC BY-NC-SA 4.0**, and that applies to MangaBaka's own data.
  - Third-party data (anything under the `source` key, and per §2 also "synopses, cover images, titles … sourced from these providers") is "made available … solely for your convenience and for personal, non-commercial use", and MangaBaka grants no licence to it.
  - FMD2r installs are personal and non-commercial. Each install fetches its own data, and FMD2r redistributes none of it, so this fits.
  - The [non-commercial terms](https://mangabaka.org/about/data-license-noncommercial) name "free, open-source projects" and "personal tools" as permitted.
  - Two things FMD2r must not do: bundle MangaBaka data in a release, or offer it as a hosted service.
- **Acceptable use** ([AUP §4.1](https://mangabaka.org/about/acceptable-use)): automated access only through the API or the dumps; respect the published rate limits; don't use the API to build "a competing service, mirror, or bulk data archive". Lazy per-title lookups, stored per list entry, are the "reasonable caching" the licence allows. Pre-fetching whole lists through the API is not; use the dump for that.
- **Covers are in the dump.** Each series has `cover.raw` (URL, size, width, height, blurhash, thumbhash) and `cover.x150` / `x250` / `x350` thumbnail URLs. The dump holds URLs, not images, so showing a cover still fetches it from `cdn.mangabaka.dev`.
  - Of 277k active comic series, 272k (98%) have a cover.
  - Original widths average 356 px; 55% are under 300 px wide, and 8% are 500 px or wider.
  - The 250-px-tall thumbnail is 185 px wide, enough for Discover's ~150 px cards. Use the 350 px one on high-DPI screens.
- **Dump size:** `series.jsonl.zst` is 388 MB (564k records, including 258k merged into others). The probe's index of titles, IDs and cover URLs is a 314 MB SQLite file, built in about a minute.
- **Links for cross-matching:** `links` holds official-site URLs. WebToons (6.7k series), Naver, Kakao, Lezhin, Tapas, Bilibili and others are common. MangaDex URLs are absent: MangaDex isn't one of its sources. A MangaDex title is reached through the cross-site IDs on the MangaDex entry instead (below).
- **Freshness:** the dump is rebuilt nightly. Each series is refreshed from its providers every 1–7 days.
- **Stability:** the docs say "No version 1.0 stability yet. The schema is subject to change"; responses carry `x-api-stability: stable` on `/v1`.

### Why not the others

- **AniList** has the richest metadata, but it is a poor fit for this:
  - 30 requests/min right now, with raises not being accepted.
  - A ban on "mass collection", and on use in "list or tracker services".
  - Lowest coverage of the lists' titles in the probe (232/400). It misses doujinshi, scanlation-only titles and most WebToons titles.
- **MangaDex** works well as a cross-ID hub. Its `links` give AniList, MU, MAL and Kitsu IDs. Its rules (proxy every image, credit MangaDex, no ads) are easy to meet. But it covers only what has been uploaded to it, and it is itself one of FMD2's websites: for the MangaDex module, `GetInfo` already uses its API and gets its 512 px covers.
- **MangaUpdates** probably covers scanlated titles best. But it has no cross-IDs, no published rate limits, small covers (~283×400) and no dump. MangaBaka already includes its data and IDs.
- **Kitsu** has good cover sizes but the smallest catalogue (63k), unreadable terms and no dump.
- **MAL**: Jikan scrapes MAL against MAL's ToS, its terms page is gone, and it was unreachable on the day. The official API needs a client ID per install. MangaBaka includes MAL IDs anyway.

## Matching

Lists hold romanized, translated and decorated titles: "(Official)", "[Colored]", "(Yaoi)", "[EN]", "(Doujinshi)". Authors are often romanized differently, e.g. "Kim Hyun-woo" vs "Kim Hyeon-woo". The probe matches in this order, stopping at the first route that gives exactly one series:

1. **Site link.** The list entry's URL is compared with the series' `links`. Used for WebToons (`title_no`): 101 of 150 sampled WebToons titles were matched this way.
2. **Cross-site IDs.** Some websites are trackers, or link to them. A MangaDex entry's `attributes.links` (`al`, `mu`, `mal`, `kt`, `ap`) is looked up against MangaBaka's `source.*.id`. This matched 101 of 150 MangaDex titles and costs one MangaDex API request per title, or one per 100 with `ids[]`. Online, MangaBaka's `/v1/source/<site>/<id>` does the same lookup.
3. **Title, then authors.**
   - Titles are normalised: NFKC, casefold, accents stripped, `&` → `and`, apostrophes dropped, other punctuation → spaces.
   - The list title and its alt titles are compared, with and without bracketed decorations, against every title MangaBaka has for a series (native, romanized, and all languages).
   - Novel entries are dropped: lists hold comics, and MangaBaka keeps a novel and its comic adaptation as separate series with different covers.
   - If the list has authors or artists, a candidate must share at least one person. Names are compared order-insensitively, with a similarity of ≥ 0.75 or the same letters in another order. This allows for romanisation differences (Hyun-woo / Hyeon-woo) and swapped syllables (Soboro / Boroso).
   - If the list names people and none of them agrees with any candidate, the match is rejected (`author-conflict`).
   - Remaining ties are broken by preferring a candidate whose main title matches.
   - A title matching more than 50 series ("Love", "Blue") is treated as ambiguous.

**Confidence threshold.** A cover is shown for `link`, `cross-id`, `title+author`, and `title-unique` (the list names no authors, and exactly one series remains). It is not shown for:

- `author-conflict`: the title matches, but the authors disagree. This is often the right series. Asura Scans, for instance, lists the original novel's author, while the comic's entry lists its adapters. But it is the tier where the wrong edition creeps in: a remake, a pre-serialization version, or the novel.
- `ambiguous`: several title matches remain.
- `none`.

In the probe, the accepted tiers had 2 wrong matches in 491 (see below). Rejected titles fall back to `GetInfo`, so a stricter threshold costs only website requests, never a wrong cover.

## Probe

Code, data and rerun instructions: [`docs/research/metadata-probe/`](metadata-probe/README.md).

**Sample:** 50 seeded titles from each of four FMD2-DB lists, per seed:

- MangaDex (102k titles): many doujinshi, Japanese romanizations, alt titles.
- MangaFire (47k): an aggregator, with "(Colored)" decorations.
- Asura Scans (320): scanlated manhwa under translated titles.
- WebToons (430): official webtoons, no authors in the list.

**Seeds.** Each seed's sample shares no titles with the earlier ones.

- **71:** the matching rules were first written against this sample.
- **72:** a held-out check. Under the first rules (`*-round1.*`), it accepted 171 (dump), 150 (MangaBaka API) and 121 (AniList), with 3, 2 and 0 wrong matches. Those errors led to the final rules: drop novel entries, and reject any author conflict.
- **73:** drawn after the rules were final, as a clean held-out sample. AniList was not rerun on it.

**Routes:** all three apply the same title and author rules ([Matching](#matching)) to their candidates.

- `dump`: the MangaBaka dump, offline, using link, then cross-ID, then title + authors.
- `mb_api`: MangaBaka's `/v1/series/match`, by title only, as a per-title lookup would do.
- `anilist`: AniList's `Media(search:)`.

### Accepted matches (final rules)

Seeds 71 / 72 / 73, with totals:

| List | MangaBaka dump | MangaBaka API | AniList (71 / 72 only) |
|---|---|---|---|
| MangaDex | 41 / 38 / 32 (111/150, 74%) | 35 / 31 / 30 (96/150, 64%) | 13 / 23 (36/100, 36%) |
| MangaFire | 45 / 42 / 45 (132/150, 88%) | 45 / 41 / 45 (131/150, 87%) | 37 / 30 (67/100, 67%) |
| Asura Scans | 38 / 32 / 38 (108/150, 72%) | 36 / 31 / 36 (103/150, 69%) | 33 / 32 (65/100, 65%) |
| WebToons | 46 / 47 / 47 (140/150, 93%) | 41 / 44 / 45 (130/150, 87%) | 26 / 38 (64/100, 64%) |
| **All** | **491/600 (82%)** | **460/600 (77%)** | **232/400 (58%)** |

How the dump's results for the 600 titles break down:

| | Count |
|---|---|
| Accepted: `link` | 101 (all WebToons) |
| Accepted: `cross-id` | 101 (all MangaDex) |
| Accepted: `title+author` | 236 |
| Accepted: `title-unique` | 53 |
| Rejected: `author-conflict` | 61 |
| Rejected: `ambiguous` | 9 |
| Rejected: `none` | 39 |

### Wrong matches (checked by hand)

**How the matches were checked:**

- **Seeds 71 and 72:** every accepted match was read side by side with the list entry (`probe.py review`). Doubtful ones were looked up in the dump (titles, people, type) and on MangaBaka.
- **Seed 73:** every accepted match whose title differs from the list's, or that rests on the title alone (`title-unique`), was checked the same way: 67 of 162. The rest pair an identical title with a matching author.
- **Automated cross-checks:**
  - Where both MangaBaka routes accepted a title, they picked the same series in 459 of 460 cases. The exception is "King of Runes" (below).
  - Where AniList and the dump both accepted a title and MangaBaka's series has an AniList ID, AniList's pick matched it in 229 of 230 cases.

**Wrong matches under the final rules:**

| | MangaBaka dump | MangaBaka API | AniList |
|---|---|---|---|
| Seed 71 | 0 of 170 | 0 of 157 | 1 of 109 |
| Seed 72 | 1 of 159 | 0 of 147 | 0 of 123 |
| Seed 73 | 1 of 162 | 0 of 156 | – |

- The two dump errors come from FMD2-DB entries whose alt titles name a different work.
  - "King of Runes" lists "King of Kung Fu • The Forbidden Kingdom" as alt titles.
  - Asura Scans' "The Time of the Terminally Ill Extra" lists the alt titles of the same author's "Bad Deeds of the Terminally Ill Empress".
  - Matching follows the list's data, so a wrong alt title gives a wrong cover.
- The API avoided both:
  - For "King of Runes" it searched by the main title only and found "Lord of the Runes". AniList's pick for that title carries the same AniList ID, so that one is right.
  - For the Asura Scans title it found the series but rejected it as an author conflict.
- One accepted match has a bad *title* but the right series: WebToons' "Not So Silent" matched a MangaBaka entry titled "unknown title (please report on Discord)". The entry's description is that webtoon's, so the cover is right.
- AniList's error is a WebToons title, "Our Time", matched to a different series of the same name.
- The first rules also accepted the wrong *edition*:
  - "Never Die Extra" was matched to its pre-serialization entry.
  - "God-Tier Extra's Ultimate Guide (Remake)" was matched to the original.
  - "Starting Today, I'm a Player" was matched to the novel.
  
  The final rules reject all three as author conflicts, so they fall back to `GetInfo`.

### Requests used

Per 200-title sample:

| Service | Requests | 429s | Spacing |
|---|---|---|---|
| MangaBaka API | 200 | 0 | 0.35 s apart |
| AniList | 200 | 1 (seed 71) | 2.1 s apart, at its 30/min limit |
| MangaDex | 50, one per MangaDex title, for the cross-IDs | 0 | 0.25 s apart |

Asking MangaBaka the same 200 questions again a few minutes later got 200 CDN cache hits, which don't count towards its limit.

For a whole list:

- **The dump route** costs the one download, plus, for the MangaDex module, one MangaDex request per 100 titles (`GET /manga?ids[]=`).
- **A per-title API route** would cost one MangaBaka request per title. Matching whole lists that way is what MangaBaka's terms forbid.
- **AniList** at 30/min would take 5.5 hours for 10,000 titles. Its terms forbid that in any case.

### What the misses are

- **MangaDex:** doujinshi and one-shots that no tracker lists. Without the cross-IDs, the API route loses 15 more MangaDex titles whose MangaDex name isn't among MangaBaka's titles.
- **Asura Scans:** mostly `author-conflict`. The list names the web novel's author, while MangaBaka's comic entry lists its adapters (and often lists the novel separately). These are usually the right series. But this is the case where editions and novels get confused, so they stay rejected and fall back to `GetInfo`.
  - T73 could accept them later through MangaBaka's `relationships` (adaptation links) once a correction UI exists.
- **MangaFire:** studio names ("Island Project"), placeholder text ("작품정보") or untransliterated names on one side.
- **WebToons:** generic one-word titles ("Blue", "Watermelon") that have no `title_no` link in MangaBaka are ambiguous.

## Follow-ups

- **T73** (new): the MangaBaka database download, offline matching (link, then cross-ID, then title + authors), format and status facets on Discover, and a description fallback on the series page.
- **T70** (updated): covers from T73's stored matches first, with `GetInfo` as the fallback. It depends on T73.
