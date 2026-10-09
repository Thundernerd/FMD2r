-- metadata.db schema v1: MangaBaka's series, reduced to what matching list titles and showing
-- their metadata need. Built in one go by MetadataBuilder and only read afterwards.
CREATE TABLE series (
    id             INTEGER PRIMARY KEY,
    title          TEXT    NOT NULL,
    type           TEXT    NOT NULL,
    status         TEXT    NOT NULL,
    year           INTEGER,
    content_rating TEXT    NOT NULL,
    description    TEXT    NOT NULL,
    -- Newline-separated.
    genres         TEXT    NOT NULL,
    -- Newline-separated normalised names of the authors and artists.
    people         TEXT    NOT NULL,
    cover_x150     TEXT,
    cover_x250     TEXT,
    cover_x350     TEXT
);
-- Every title of a series (main, native, romanized, other languages), normalised.
CREATE TABLE titles (
    key    TEXT    NOT NULL,
    series INTEGER NOT NULL,
    PRIMARY KEY (key, series)
) WITHOUT ROWID;
-- Links to sites whose IDs are stable, as `<site>:<id>`.
CREATE TABLE links (
    key    TEXT    NOT NULL,
    series INTEGER NOT NULL,
    PRIMARY KEY (key, series)
) WITHOUT ROWID;
-- IDs on other sites (MangaBaka's `source` keys: anilist, manga_updates, my_anime_list, ...).
CREATE TABLE xids (
    site   TEXT    NOT NULL,
    xid    TEXT    NOT NULL,
    series INTEGER NOT NULL,
    PRIMARY KEY (site, xid, series)
) WITHOUT ROWID;
-- Merged series while building; emptied once their titles and IDs point at the final series.
CREATE TABLE merged (
    src INTEGER PRIMARY KEY,
    dst INTEGER NOT NULL
);
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
