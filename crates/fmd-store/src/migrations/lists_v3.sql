-- lists.db schema v3: each list title's match in MangaBaka's database (metadata.db, T73).
-- `series_id` is set only for an accepted match; a rejected one keeps its confidence and no
-- series. `format` and `status` are the matched series' Discover facet values, copied so the
-- search needs no second database. `fingerprint` is what the title was matched on, so a list
-- change re-matches only the titles it touched.
CREATE TABLE metadata_matches (
    module_id   TEXT    NOT NULL,
    link        TEXT    NOT NULL,
    series_id   INTEGER,
    confidence  TEXT    NOT NULL,
    format      TEXT,
    status      TEXT,
    year        INTEGER,
    fingerprint TEXT    NOT NULL,
    PRIMARY KEY (module_id, link)
) WITHOUT ROWID;
