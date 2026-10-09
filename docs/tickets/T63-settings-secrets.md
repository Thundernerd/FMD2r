# T63: Keep settings secrets out of the API and encrypt them at rest
Deps: none

## Goal
Decided in T51 (item 1). The proxy password (`ProxySettings.password`, `crates/fmd-core/src/settings/model.rs:151`, and the per-module `ProxyOverride.password`, `crates/fmd-core/src/settings/module_overrides.rs:256`), the server password (`ServerSettings.auth_token`, `model.rs:477-478`) and the GitHub token (`ModuleUpdaterSettings.github_token`, `model.rs:449-450`) are plain JSON in `app.db` (`crates/fmd-store/src/app/settings.rs:21-53`). `GET /api/settings`, `PATCH /api/settings` and `/api/settings/all` return them unredacted (`crates/fmd-server/src/settings.rs:36-41`); the UI only masks them (`web/src/lib/settings/sections.ts:202`, :248). Accounts already do both right (`crates/fmd-core/src/accounts.rs:59-67`, `redact()` at :348).

## Scope (in/out)
In:
- The settings API and the module settings API never return a secret. Each secret becomes a `has_<name>` flag (as accounts' `has_password`). A PATCH that leaves the field out keeps the stored value; an empty string clears it.
- Secrets are encrypted at rest with the existing `KeyFileCipher` (`crates/fmd-store/src/crypto.rs:27-99`) and the same key file (`accounts.key`); no second key file.
- Plain values already in `app.db` are encrypted on the first start after the upgrade.
- The Settings UI shows "set" / "not set" for a secret and sends a value only when the user types one.
- Update `openapi.json` and the web schema.

Out: the server password's hashing and use (T64), which stores a hash instead of an encrypted value.

## Seams under test
- `fmd-server`: `GET /api/settings` after storing each secret returns no secret value and `has_*: true`; a PATCH without the field keeps it; a PATCH with `""` clears it.
- `fmd-core` / `fmd-store`: the stored row for each secret contains no plain text; a database with plain values is migrated on start, and the values still work (e.g. the proxy is still used).
- Web component test: a set secret shows as set, and saving another field does not send it.

## Acceptance criteria
- [ ] No API response contains a secret.
- [ ] No secret is stored in plain text in `app.db`.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks and tests pass.

## FMD2 references
None (FMD2 stores these in plain `settings.json`; this is a server-side hardening).
