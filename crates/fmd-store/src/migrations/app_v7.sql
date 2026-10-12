-- app.db schema v7: library groups (issue #257). Each favorite is in at most one group, like a
-- folder; deleting a group ungroups its favorites. `collapsed` is the section's fold state, kept
-- here so it is the same on every device.
CREATE TABLE library_groups (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    sort_order INTEGER NOT NULL,
    collapsed  INTEGER NOT NULL DEFAULT 0
);

ALTER TABLE favorites ADD COLUMN group_id INTEGER REFERENCES library_groups (id) ON DELETE SET NULL;
