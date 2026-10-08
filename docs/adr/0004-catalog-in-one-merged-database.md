# Ingest FMD2-DB lists into one merged catalog database

FMD2 keeps one SQLite file per website (`data/<module>.db`). It replaces the file wholesale when it downloads that website's FMD2-DB list, and it `ATTACH`es up to 125 of them when the user browses several websites. FMD2r instead ingests every website into a single `catalog.db`, separate from the user-state `fmd2r.db`. The FMD2-DB per-site schema stays the **import format**, not the query format. Discover filters several websites server-side with facet counts and cursor pagination. That needs indexes spanning websites (normalized genres, FTS5 titles), which separate attached files can't provide.

## Considered Options

- **Per-site files plus `ATTACH`**: mirrors upstream. But it is capped at 10 attachments by default, facet counts need a `UNION ALL` across every website, and no index can span websites.
- **Catalog tables inside `fmd2r.db`**: one file, but backups grow by the size of a cache that can be rebuilt at any time.

## Consequences

- All three sources upsert by (module, link), one transaction per website: an FMD2-DB download, an update-list run, and refreshing a series' info page. First-seen keeps the earlier date, and rows are never deleted. An FMD2-DB download therefore no longer erases rows the user's own list updates added, and it doesn't reset "new".
- Genres are normalized into whole case-folded tokens. Excludes always remove a series that has any excluded genre. This deliberately departs from FMD2's substring `LIKE` matching and its "any"-mode exclude quirk.
- `catalog.db` is a cache. It is never migrated or backed up: on a schema change it is dropped and re-ingested.
