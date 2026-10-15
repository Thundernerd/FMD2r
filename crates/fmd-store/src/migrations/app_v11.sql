-- app.db schema v11: inbox items can be deleted (issue #306). SQLite reuses the highest rowid
-- after that row is deleted, and SSE clients resume after the last inbox id they saw, so the
-- table is rebuilt with AUTOINCREMENT to never hand out a deleted id again.
CREATE TABLE events_v11 (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    ts        INTEGER NOT NULL,
    kind      TEXT    NOT NULL,
    severity  TEXT    NOT NULL,
    module_id TEXT,
    task_id   INTEGER,
    title     TEXT    NOT NULL,
    body      TEXT    NOT NULL DEFAULT 'null',
    read      INTEGER NOT NULL DEFAULT 0
);
INSERT INTO events_v11 (id, ts, kind, severity, module_id, task_id, title, body, read)
    SELECT id, ts, kind, severity, module_id, task_id, title, body, read FROM events;
DROP TABLE events;
ALTER TABLE events_v11 RENAME TO events;
CREATE INDEX events_ts ON events (ts);
CREATE INDEX events_unread ON events (read, ts);
CREATE INDEX events_module_id ON events (module_id);
