# T64: Make the Password and Listen address settings work
Deps: T63

## Goal
Decided in T51 (item 2). The Settings page offers a "Password" and a "Listen address" (`web/src/lib/settings/sections.ts:247-248`), but the server reads neither: authentication only comes from `--password` / `FMD2R_PASSWORD` (`crates/fmd2r/src/main.rs:57-58`, :122), and the address only from `--bind` / `FMD2R_BIND` (`main.rs:50-52`, default `127.0.0.1:8080`). The settings default for `server.bind` is `0.0.0.0:8080` (`crates/fmd-core/src/settings/model.rs:491`). Nothing warns when the server runs without a password.

## Scope (in/out)
In:
- **Password:** the server uses `settings.server.auth_token` when set. `--password` / `FMD2R_PASSWORD` overrides it. The setting is stored as a salted password hash (e.g. Argon2), not the password; the API reports `has_password` (see T63). Bearer tokens and `POST /api/login` verify against the hash; cache a verified bearer token so a request doesn't pay for a hash. Sessions stay bound to the secret (`crates/fmd-server/src/auth.rs:43-49`): bind them to the stored hash, so changing the password ends them. A change applies without a restart.
- **Listen address:** the server binds `settings.server.bind`; `--bind` / `FMD2R_BIND` overrides it. Change the settings default to `127.0.0.1:8080` to match the CLI. Docker keeps `FMD2R_BIND=0.0.0.0:8080` (`Dockerfile:77`). It takes effect after a restart, as the UI already says.
- **Warning:** with no password and a non-loopback address, log a warning at startup, and the UI shows a banner (from `GET /api/health`, which already reports `auth`, `crates/fmd-server/src/health.rs:23`).
- The UI says when the CLI or environment overrides a setting.

Out: users, roles, or more than one password.

## Seams under test
- `fmd-server`: a password set through `PATCH /api/settings` is required by the next request, without a restart; the env password wins over the setting; the stored value is not the password; changing it ends existing sessions.
- `fmd2r`: the bind address comes from the setting when no flag or env is given, and from the flag otherwise.
- The startup warning is logged for `0.0.0.0` without a password and not for `127.0.0.1`.
- Web component test: the banner shows when `/api/health` reports no auth on a non-loopback address.

## Acceptance criteria
- [ ] Both settings take effect; the CLI and environment still override them.
- [ ] The password is stored only as a hash.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks and tests pass.

## FMD2 references
None (FMD2 is a desktop app with no server).
