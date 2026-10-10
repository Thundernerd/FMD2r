-- lists.db schema v4: each list title's cover link, once known (T70), so a title's info page is
-- fetched once per revalidation period rather than once per view. A list has no cover column
-- (FMD2's per-site tables don't either, baseunits/DBDataProcess.pas:143-153), so this is kept
-- apart from `masterlist` and outlives list updates and imports.
-- `url` is NULL when the title is known to have no cover. `source` is `mangabaka` (the title's
-- accepted match in metadata.db, `series_id`, with its 250 px and 350 px tall thumbnails in `url`
-- and `large_url`) or `website` (the module's `GetInfo`). `checked_at` is in Unix seconds.
CREATE TABLE cover_links (
    module_id  TEXT    NOT NULL,
    link       TEXT    NOT NULL,
    url        TEXT,
    large_url  TEXT,
    source     TEXT    NOT NULL,
    series_id  INTEGER,
    checked_at INTEGER NOT NULL,
    PRIMARY KEY (module_id, link)
) WITHOUT ROWID;
