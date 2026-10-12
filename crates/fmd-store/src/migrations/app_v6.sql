-- app.db schema v6: MangaBaka series picked by hand for a series (issue #249). Kept here, not in
-- lists.db, so they outlive rebuilding the lists and removing the MangaBaka database; matching
-- applies them in place of its own match.
CREATE TABLE metadata_overrides (
    module_id TEXT    NOT NULL,
    link      TEXT    NOT NULL,
    series_id INTEGER NOT NULL,
    PRIMARY KEY (module_id, link)
) WITHOUT ROWID;
