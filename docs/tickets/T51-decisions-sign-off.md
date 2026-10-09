# T51: Sign off decisions flagged in the merged PRs
Deps: none
Owner: user

## Goal
Decisions the ticket agents made and asked to have reviewed. This ticket is for the maintainer, not an agent: the coordinator script skips tickets with `Owner: user`. For each item, accept the current behaviour or turn the recommendation into a ticket.

## Items
1. **Secrets in plain text (T18, PR #31).** Proxy credentials, the server auth token and the GitHub token are plain JSON in `app.db`. Recommendation: encrypt them with the same `Cipher` and key file as accounts (T31), and keep the server password as a hash.
2. **Default bind `0.0.0.0:8080` with no auth (T18/T21).** Recommendation: keep it for Docker, but log a warning on startup and show a banner in the UI when no password is set.
3. **Adding a favorite marks its current chapters downloaded (T25, PR #56).** The series page then shows them as downloaded although no files exist. Recommendation: store "seen" chapters separately from downloaded chapters, which also fixes the undercounting new-chapter badge.
4. **Single output format per install (T23, PR #57).** The per-task format was removed to match FMD2. Recommendation: accept.
5. **Real "Disabled" status and lazy `DynamicPageLink` (T20, PR #53).** Recommendation: accept; only EHentai uses `DynamicPageLink`.
6. **Threads-per-task override honoured (T18).** FMD2 ignores it. Recommendation: accept.
7. **Process-wide SIGFPE handler in `libfmdxpath.so` (T07, PR #29).** Only matters when the fpc backend is loaded, which is no longer the default. Recommendation: accept.
8. **TStrings `Text` joins with CRLF (T04, PR #33)**, as FMD2's Windows build. Recommendation: accept; modules were written against it.
9. **`fmd.pcre2` is a stub (T13).** No upstream module uses it. Recommendation: accept; the module scan (T29) reports it if one ever does.

## Acceptance criteria
- [x] Each item marked accepted, or a follow-up ticket created for it.

## Decisions (2026-10-09)
1. Follow-up: T63 (the API never returns secrets; encrypted at rest with `accounts.key`). The check found the API returned them unredacted.
2. Follow-up: T64. The check found the Password and Listen address settings had no effect (only `--password` / `--bind` and their env vars did) and the real default bind is `127.0.0.1:8080`. T64 makes the settings work (stored password hashed, CLI and env override), aligns the default, and warns when there is no password on a non-loopback address.
3. The display is decided in T60 (call the mark "seen") and done in T62. Storing seen chapters separately would not fix the badge, which compares two counts; follow-up: T65 (count by link).
4. Accepted.
5. Accepted.
6. Accepted.
7. Accepted.
8. Accepted.
9. Accepted.
