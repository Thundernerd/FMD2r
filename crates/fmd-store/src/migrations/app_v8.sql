-- app.db schema v8: the website's genres of a favorite (issue #263), so its tags are known
-- without fetching the website again. Written with the v5 columns whenever the website's info is
-- fetched; empty until then. Not part of what MangaBaka matching reads.
ALTER TABLE favorites ADD COLUMN genres TEXT NOT NULL DEFAULT '';
