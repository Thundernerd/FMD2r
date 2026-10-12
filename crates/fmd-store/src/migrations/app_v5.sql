-- app.db schema v5: what the website says about a favorite, as MangaBaka matching reads it
-- (issue #242), so favorites match without their website's list. Kept apart from `title`,
-- which the user can rename. Written whenever the website's info is fetched; empty until then.
ALTER TABLE favorites ADD COLUMN info_title TEXT NOT NULL DEFAULT '';
ALTER TABLE favorites ADD COLUMN alt_titles TEXT NOT NULL DEFAULT '';
ALTER TABLE favorites ADD COLUMN authors    TEXT NOT NULL DEFAULT '';
ALTER TABLE favorites ADD COLUMN artists    TEXT NOT NULL DEFAULT '';
