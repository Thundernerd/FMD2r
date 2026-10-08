-- app.db schema v1. Timestamps are Unix milliseconds (INTEGER); booleans are 0/1.

-- Replaces FMD2's `downloads` table (baseunits/DownloadsDB.pas:67-92).
CREATE TABLE tasks (
    id                   INTEGER PRIMARY KEY,
    module_id            TEXT    NOT NULL,
    link                 TEXT    NOT NULL,
    title                TEXT    NOT NULL,
    save_to              TEXT    NOT NULL,
    status               TEXT    NOT NULL,
    enabled              INTEGER NOT NULL DEFAULT 1,
    sort_order           INTEGER NOT NULL,
    date_added           INTEGER NOT NULL,
    date_last_downloaded INTEGER,
    current_chapter      INTEGER NOT NULL DEFAULT 0,
    error                TEXT
);
CREATE INDEX tasks_status ON tasks (status);
CREATE INDEX tasks_module_id ON tasks (module_id);
CREATE INDEX tasks_sort_order ON tasks (sort_order);

-- Replaces the newline-joined chapterslinks/chaptersnames/customfilenames/chaptersstatus columns.
CREATE TABLE task_chapters (
    task_id         INTEGER NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    idx             INTEGER NOT NULL,
    link            TEXT    NOT NULL,
    name            TEXT    NOT NULL,
    custom_filename TEXT,
    status          TEXT    NOT NULL,
    page_count      INTEGER NOT NULL DEFAULT 0,
    current_page    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (task_id, idx)
) WITHOUT ROWID;

-- Replaces the newline-joined pagelinks/pagecontainerlinks/filenames columns.
CREATE TABLE task_pages (
    task_id       INTEGER NOT NULL,
    chapter_idx   INTEGER NOT NULL,
    idx           INTEGER NOT NULL,
    url           TEXT    NOT NULL,
    container_url TEXT    NOT NULL,
    filename      TEXT    NOT NULL,
    status        TEXT    NOT NULL,
    PRIMARY KEY (task_id, chapter_idx, idx),
    FOREIGN KEY (task_id, chapter_idx) REFERENCES task_chapters (task_id, idx) ON DELETE CASCADE
) WITHOUT ROWID;

-- Replaces FMD2's `favorites` table (baseunits/FavoritesDB.pas:55-71).
CREATE TABLE favorites (
    id                INTEGER PRIMARY KEY,
    module_id         TEXT    NOT NULL,
    link              TEXT    NOT NULL,
    title             TEXT    NOT NULL,
    status            TEXT    NOT NULL DEFAULT '',
    current_chapter   INTEGER NOT NULL DEFAULT 0,
    save_to           TEXT    NOT NULL,
    enabled           INTEGER NOT NULL DEFAULT 1,
    sort_order        INTEGER NOT NULL,
    date_added        INTEGER NOT NULL,
    date_last_checked INTEGER,
    date_last_updated INTEGER,
    cover_url         TEXT,
    UNIQUE (module_id, link)
);
CREATE INDEX favorites_sort_order ON favorites (sort_order);

-- One row per downloaded chapter instead of FMD2's one-row-per-manga text blob
-- (baseunits/DownloadedChaptersDB.pas:124-129). FMD2 matches case-insensitively (:70, :73).
CREATE TABLE downloaded_chapters (
    module_id    TEXT NOT NULL COLLATE NOCASE,
    manga_link   TEXT NOT NULL COLLATE NOCASE,
    chapter_link TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (module_id, manga_link, chapter_link)
) WITHOUT ROWID;

-- Per-module settings (baseunits/WebsiteModulesSettings.pas, modules.json in WebsiteModules.pas:545-696).
CREATE TABLE module_settings (
    module_id  TEXT PRIMARY KEY,
    enabled    INTEGER NOT NULL DEFAULT 0,
    options    TEXT    NOT NULL DEFAULT '{}',
    http       TEXT    NOT NULL DEFAULT '{}',
    limits     TEXT    NOT NULL DEFAULT '{}',
    cookie_jar BLOB
);

-- Module accounts; username/password/cookies hold ciphertext.
CREATE TABLE accounts (
    module_id TEXT PRIMARY KEY,
    enabled   INTEGER NOT NULL DEFAULT 0,
    username  BLOB    NOT NULL,
    password  BLOB    NOT NULL,
    cookies   BLOB    NOT NULL,
    status    TEXT    NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Inbox and history.
CREATE TABLE events (
    id        INTEGER PRIMARY KEY,
    ts        INTEGER NOT NULL,
    kind      TEXT    NOT NULL,
    severity  TEXT    NOT NULL,
    module_id TEXT,
    task_id   INTEGER,
    title     TEXT    NOT NULL,
    body      TEXT    NOT NULL DEFAULT 'null',
    read      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX events_ts ON events (ts);
CREATE INDEX events_unread ON events (read, ts);
CREATE INDEX events_module_id ON events (module_id);

-- Lua files synced from upstream.
CREATE TABLE module_files (
    path          TEXT PRIMARY KEY,
    sha           TEXT    NOT NULL,
    last_modified INTEGER NOT NULL,
    size          INTEGER NOT NULL
);
