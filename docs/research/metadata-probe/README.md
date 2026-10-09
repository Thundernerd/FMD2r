# T71 metadata probe

Throwaway research code for `docs/research/metadata-sources.md`. It is not part of FMD2r and nothing builds or tests it.

`probe.py` matches seeded samples of real FMD2-DB lists against MangaBaka (its database dump and its API) and AniList, and writes the results here:

- `results-<seed>.csv`: one row per sampled title, giving each route's confidence, matched ID, matched title and cover URL.
- `summary-<seed>.json`: accepted matches per list and per confidence, and the HTTP requests each service received.

Seed 71 is the sample the matching rules were first tuned on. Seeds 72 and 73 are held-out samples; each has no titles in common with the samples drawn before it. Seed 73 was drawn after the rules were final, and only the two MangaBaka routes were run on it.

The checks on seed 72 found wrong matches that led to the final rules: novel entries are dropped, any author conflict is rejected, names may match with syllables swapped, and alt titles are split on the list's own separator. `*-round1.*` are the results under the first rules. The unsuffixed files are the current rules over the same samples. API responses are cached in `work/http-cache/`, so rerunning `match` with changed rules sends no requests. The `requests` counts in a summary cover only what went over the network, so a rerun from the cache shows none. The counts for the first runs are in `docs/research/metadata-sources.md` ("Requests used") and in the `round1` summaries.

## Rerunning

Needs Python 3 (standard library only, with SQLite 3.33 or later), `curl`, `zstd` and `7z`. About 700 MB of disk goes to `work/` (git-ignored).

```sh
cd docs/research/metadata-probe
python3 probe.py index              # ~390 MB MangaBaka dump -> work/mangabaka.sqlite (~1 min)
python3 probe.py sample             # FMD2-DB dumps of 4 lists -> work/sample-71.json
python3 probe.py match              # ~10 min, AniList's 30/min limit dominates
python3 probe.py review             # accepted matches side by side, for the hand check
python3 probe.py sample --seed 72   # held-out samples, drawn after seed 71
python3 probe.py match --seed 72
python3 probe.py sample --seed 73
python3 probe.py match --seed 73 --routes dump,mb_api
```

`match --routes dump` skips the two online services. The `dump` route still makes one MangaDex API call per MangaDex title, to read its cross-site IDs.

The lists and services change over time, so a rerun will not reproduce the numbers exactly. The sample is fixed by the seed and each list's link order, but FMD2-DB and MangaBaka both update daily.

## Data and terms

- FMD2-DB dumps: <https://github.com/dazedcat19/FMD2-DB>, GPL-2.0.
- The MangaBaka dump (<https://mangabaka.org/data/database>) is under MangaBaka's data licence: CC BY-NC-SA 4.0 for MangaBaka's own data, and each provider's own terms for third-party data. The probe keeps it in `work/`. Only titles, IDs and cover URLs for the 400 sampled titles end up in the committed CSVs.
- About 700 requests per run: 200 to MangaBaka (rate limit 180/min), 200 to AniList (30/min), 50 to MangaDex (5/s). All are spaced to stay under those limits.
