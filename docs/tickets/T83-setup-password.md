# T83: Setup step: password, when the server is open
Deps: T78

## Goal
A server reachable from other machines without a password can be used by anyone who reaches it. FMD2r already detects this: `GET /api/health` reports `auth` and `loopback` (`crates/fmd-server/src/health.rs:15-17`, :27-28), and the "anyone who can reach it can use it" banner shows when there is no password on a non-loopback address (`web/src/lib/components/OpenServerBanner.svelte`). Docker binds `0.0.0.0` by default, so a fresh container is in exactly that state. The setup (T78) should offer to set a password then, and stay out of the way otherwise.

## Scope (in/out)
In:
- A "Password" step that shows only when the server is open (the banner's condition: `!health.auth && !health.loopback`). On a loopback-only server, or one that already has a password, the step is absent.
- It explains why it's there (reachable from other machines, no password) and offers a password field with a confirmation field. Saving sets `server.auth_token`, stored as a hash (T64).
- **The password is set by the environment:** when `server.auth_token` is in `health.overridden` (`--password` / `FMD2R_PASSWORD`), the step doesn't show, as there is nothing to set.
- **Skippable:** "Skip" leaves the server open; the banner keeps warning, as today.
- **No logout mid-setup:** setting the password ends existing sessions (T64), so the step comes last, just before the finish step. After saving, it logs in with the new password (`POST /api/login`) so the user stays in the wizard instead of landing on the login screen.

Out: a listen-address step (`server.bind` applies only after a restart, Docker's `FMD2R_BIND` overrides it, and a wrong choice can lock the user out); users or roles.

## Seams under test
- Web component tests: the step is present when health reports no auth on a non-loopback address, and absent for loopback, for an existing password, and when `server.auth_token` is overridden; mismatched confirmation keeps Next disabled; saving patches `server.auth_token` and then logs in.
- Playwright (mocked API): an open server shows the step; setting a password finishes setup without showing the login screen, and the banner is gone afterwards.

## Acceptance criteria
- [ ] An open server's setup offers to set a password, and setting it doesn't interrupt the setup.
- [ ] The step never shows when a password isn't needed or can't be set from the UI.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None (FMD2 is a desktop app with no server).
