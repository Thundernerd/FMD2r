-- app.db schema v9: failed checks of a favorite (issue #273), so a series whose website keeps
-- failing is flagged and checked less often. `check_failures` counts consecutive failed info
-- fetches, `last_check_error` is the last one's message and `date_last_failed` (Unix ms) its
-- time; a successful check clears all three.
ALTER TABLE favorites ADD COLUMN check_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE favorites ADD COLUMN last_check_error TEXT;
ALTER TABLE favorites ADD COLUMN date_last_failed INTEGER;
