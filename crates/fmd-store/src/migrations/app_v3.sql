-- app.db schema v3: the chapter links the site listed at a favorite's last check (T65), so the
-- new-chapter badge can compare links with `downloaded_chapters` instead of counts. FMD2 stores
-- only the count (`currentchapter`); favorites not checked since this version have no rows.
CREATE TABLE favorite_chapters (
    favorite_id INTEGER NOT NULL REFERENCES favorites (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    link        TEXT    NOT NULL COLLATE NOCASE,
    PRIMARY KEY (favorite_id, position)
) WITHOUT ROWID;
