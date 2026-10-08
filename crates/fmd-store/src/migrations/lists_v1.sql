-- lists.db schema v1: FMD2's per-site list tables (baseunits/DBDataProcess.pas:143-153) merged
-- into one table keyed by module, which avoids FMD2's 125-ATTACH limit.
-- `id` is an explicit INTEGER PRIMARY KEY so VACUUM cannot renumber the rowids the external-content
-- FTS index points at; (module_id, link) is the natural key.
CREATE TABLE masterlist (
    id         INTEGER PRIMARY KEY,
    module_id  TEXT    NOT NULL,
    link       TEXT    NOT NULL,
    title      TEXT    NOT NULL DEFAULT '',
    alttitles  TEXT    NOT NULL DEFAULT '',
    authors    TEXT    NOT NULL DEFAULT '',
    artists    TEXT    NOT NULL DEFAULT '',
    genres     TEXT    NOT NULL DEFAULT '',
    status     TEXT    NOT NULL DEFAULT '',
    summary    TEXT    NOT NULL DEFAULT '',
    numchapter INTEGER NOT NULL DEFAULT 0,
    added_jdn  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (module_id, link)
);
CREATE INDEX masterlist_title ON masterlist (module_id, title COLLATE NOCASE);
-- Matches the search order when results span modules.
CREATE INDEX masterlist_title_all ON masterlist (title COLLATE NOCASE, module_id, link);
CREATE INDEX masterlist_added_jdn ON masterlist (module_id, added_jdn);

CREATE VIRTUAL TABLE masterlist_fts USING fts5 (
    title, alttitles, authors, artists, genres, summary,
    content = 'masterlist',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);

-- MasterListRepo::replace_module drops masterlist_ai/masterlist_ad inside its transaction and
-- recreates them from sqlite_master afterwards, syncing the index with set-based statements.
CREATE TRIGGER masterlist_ai AFTER INSERT ON masterlist BEGIN
    INSERT INTO masterlist_fts (rowid, title, alttitles, authors, artists, genres, summary)
    VALUES (new.id, new.title, new.alttitles, new.authors, new.artists, new.genres, new.summary);
END;

CREATE TRIGGER masterlist_ad AFTER DELETE ON masterlist BEGIN
    INSERT INTO masterlist_fts (masterlist_fts, rowid, title, alttitles, authors, artists, genres, summary)
    VALUES ('delete', old.id, old.title, old.alttitles, old.authors, old.artists, old.genres, old.summary);
END;

CREATE TRIGGER masterlist_au AFTER UPDATE ON masterlist BEGIN
    INSERT INTO masterlist_fts (masterlist_fts, rowid, title, alttitles, authors, artists, genres, summary)
    VALUES ('delete', old.id, old.title, old.alttitles, old.authors, old.artists, old.genres, old.summary);
    INSERT INTO masterlist_fts (rowid, title, alttitles, authors, artists, genres, summary)
    VALUES (new.id, new.title, new.alttitles, new.authors, new.artists, new.genres, new.summary);
END;
