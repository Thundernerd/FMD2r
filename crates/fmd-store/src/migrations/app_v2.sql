-- app.db schema v2: web UI login sessions (T41). No FMD2 counterpart.

-- One row per session cookie. The cookie value itself is never stored, only a hash of it bound
-- to the configured password, so a leaked database yields no usable cookies.
CREATE TABLE sessions (
    token_hash BLOB    PRIMARY KEY,
    created_at INTEGER NOT NULL,
    last_seen  INTEGER NOT NULL
) WITHOUT ROWID;
