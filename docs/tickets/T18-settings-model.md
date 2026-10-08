# T18: Settings model
Deps: T17

## Goal
A typed settings model with FMD2-compatible defaults, persisted in `app.db`'s `settings` table, plus per-module overrides, so the engine, jobs and UI read one source of truth.

## Scope (in/out)
In:
- `Settings` struct tree (serde) grouped like FMD2's options: general (data dirs, language), connections (max parallel tasks, threads per task, retry count, timeout, global proxy, user agent), saveto (default directory, filename/chapter/manga rename templates `%MANGA%`/`%CHAPTER%`/… with FMD2 defaults, remove unicode, convert digit volume/chapter, digit padding, illegal-character stripping mode with a POSIX default), output format (folder/zip/cbz/pdf/epub, PDF quality), image conversion (PNG→JPEG, WebP→PNG/JPEG, quality, optional ImageMagick), favorites (check interval, check at startup, auto-download new chapters), update lists (auto, interval), module updater (auto, interval, repo owner/name/ref/path), server (bind address, auth token/password).
- Defaults copied from FMD2 (`FMDOptions.pas` constants, `frmMain.LoadOptions` default arguments); each default cites its source line.
- `SettingsService::{load, get, update(patch)}` with validation (ranges, enum values) and change notifications (a `tokio::sync::watch` or broadcast) for live reconfiguration.
- Per-module overrides (`ModuleOverrides`: enabled, max task limit, threads per task, max connections, UA, cookies, proxy, option values) stored via `ModuleSettingsRepo`, with an `effective_limits(module_def, overrides, global)` function implementing FMD2's precedence.
- JSON Schema or OpenAPI-friendly derives (`utoipa::ToSchema`) so T21/T27 can expose and render it.

Out: the settings UI (T27); FMD2 import of `settings.json` (T32 uses this model).

## Seams under test
Public `fmd-store`/`fmd-core` settings API (wherever it lives) on a temp DB:
- Fresh DB → `get()` returns FMD2 defaults (assert a handful of specific values with their Pascal citations, e.g. default manga rename `%MANGA%`, filename `%FILENAME%`).
- `update` with an invalid value (negative threads) → validation error, nothing persisted.
- Partial patch updates only given keys; subscribers notified once.
- `effective_limits`: module default 1 task, global 4, override 2 → 2; override absent → module value; module 0 (unlimited) → global.

## Acceptance criteria
- [ ] All option groups above modelled; defaults documented with FMD2 citations.
- [ ] Round-trip through `settings` table is lossless and forward-compatible (unknown keys preserved or ignored, documented).
- [ ] Limit precedence matches `WebsiteModules.pas:398-420` and `WebsiteModulesSettings.pas`.

## FMD2 references
- `baseunits/FMDOptions.pas:20-60` (default constants incl. `DEFAULT_MANGA_CUSTOMRENAME`, `DEFAULT_FILENAME_CUSTOMRENAME`), `:259-300` (paths)
- `mangadownloader/forms/frmMain.pas:5803-5980` (`LoadOptions`: every option key and its default), `:5981-6168` (`SaveOptions`), `:6169-6389` (`ApplyOptions`)
- `baseunits/WebsiteModulesSettings.pas:94-171` (per-module settings: enabled, HTTP overrides, limits)
- `baseunits/WebsiteModules.pas:353-420` (`PrepareHTTP`, `GetMaxTaskLimit`, `GetMaxThreadPerTaskLimit`, `CanCreateTask`)
- `baseunits/uBaseUnit.pas:250-257` (rename tokens), `:1798-1830` (`CustomRename` option use)
