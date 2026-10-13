-- app.db schema v9: auto groups (issue #264). A group with a rule collects the series in no group
-- whose tags hold `rule_value` ('tag') or whose MangaBaka format is `rule_value` ('format').
-- `rule_kind` NULL is a group filled by hand only.
ALTER TABLE library_groups ADD COLUMN rule_kind TEXT;
ALTER TABLE library_groups ADD COLUMN rule_value TEXT;
