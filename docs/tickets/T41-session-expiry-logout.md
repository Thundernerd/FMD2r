# T41: Session expiry and logout
Deps: none

## Goal
With a password set, `POST /api/login` issues an HttpOnly session cookie that never expires and can't be revoked; there is no logout (PR #35, "Not done"). Add expiry, logout and a way to end all sessions.

## Scope (in/out)
In:
- Server-side sessions (in `app.db`) with an idle timeout and absolute lifetime (settings with sensible defaults, e.g. 30 days absolute, 7 days idle), sliding renewal, `Secure` when served over HTTPS (or behind a proxy that says so).
- `POST /api/logout` (current session) and `POST /api/sessions/revoke-all`; changing the password revokes all sessions.
- Web: a logout entry and a login screen when the API answers 401.
- Bearer token auth keeps working unchanged.

Out: multi-user accounts.

## Seams under test
HTTP handlers via `oneshot` with an injectable clock: login → request OK → advance past idle timeout → 401; logout → old cookie gets 401; revoke-all → every cookie gets 401; password change revokes. Playwright (mock): 401 shows the login screen, logout returns to it.

## Acceptance criteria
- [ ] Constant-time comparisons kept; session ids are random 128-bit+ values stored hashed.
- [ ] fmt, clippy, tests and web checks pass.

## FMD2 references
- None (FMD2 has no web server).
