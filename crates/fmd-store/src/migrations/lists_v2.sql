-- lists.db schema v2: when each module's list last changed through MasterListRepo::replace_module
-- or MasterListRepo::insert_new (Unix milliseconds), for the Discover page's website picker.
CREATE TABLE list_updates (
    module_id  TEXT    PRIMARY KEY,
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;
