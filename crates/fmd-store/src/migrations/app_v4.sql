-- app.db schema v4: a module's download folder (T74), FMD2's `OverrideSettings.SaveToPath`
-- (baseunits/WebsiteModulesSettings.pas:50). Empty means the default destination.
ALTER TABLE module_settings ADD COLUMN save_to TEXT NOT NULL DEFAULT '';
