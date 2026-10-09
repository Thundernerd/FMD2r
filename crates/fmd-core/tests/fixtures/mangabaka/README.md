# MangaBaka dump fixture

`series.jsonl` holds 15 records of MangaBaka's nightly dump (`https://api.mangabaka.org/v1/database/series.jsonl.zst`, recorded on 2026-10-09), one JSON object per line as the dump has them. Records 74312, 375994, 56517, 49665, 83451, 7114 and 19054 were recorded from `api.mangabaka.org/v1/series/...`, which returns the same objects.

They are the series behind the T71 probe cases the T73 tests use (`docs/research/metadata-probe/results-*.csv`): Reborn Rich (WebToons link), Reborn as a Scholar (MangaDex cross-IDs), Shadow Star☆ and Revenge of the Baskerville Bloodhound (author romanisations), Shaberi Sugita Onnanoko ("(Colored)"), the Never Die Extra editions and novel, From Today, I'm a Player (a novel), two series titled Blue, and ONE PIECE with the colored edition merged into it (728).

To keep the file small, some fields were trimmed: `tags_v2`, `links_v2`, `relationships_v2`, `popularity`, `publishers`, `genres_v2`, `relationships`, `anime` and `published` are dropped, `source` keeps only each provider's `id` and `rating`, `cover.raw` keeps its URL and size, and long descriptions, tag and secondary-title lists are cut. The fields the build reads are as recorded, and the extra ones left in (`tags`, `rating`, `canonical_url`, ...) check that unknown fields are tolerated.

One record is edited: Failed Princesses (952) has `"state": "deleted"`, since the recorded part of the dump holds no deleted series.

The data is MangaBaka's, under its data licence (CC BY-NC-SA 4.0, <https://mangabaka.org/about/data-license>); third-party fields keep their providers' terms.
