# T39: Real-server end-to-end run in CI
Deps: T38

## Goal
Every handler is tested on its own, but nothing in CI runs the real composed server, which is how the wiring gaps fixed in T37/T38 went unnoticed. Run `npm run test:e2e:real` (T23) in CI against a real `fmd2r serve` and extend it to the plan's end-to-end flow.

## Scope (in/out)
In:
- New CI job (in `ci.yml`, path-filtered like `docker.yml` if useful) with both the Rust toolchain and Node: build `web/`, build `fmd2r`, run `web/playwright.real.config.ts`.
- Extend the real-server Playwright suite (desktop + 375px) to: add-by-URL → series page with cover → add to library → queue 2 chapters → live progress over SSE without reload → restart the server mid-download → both chapters finish → Get files returns a valid CBZ with pages in natural order → "Check for new chapters" posts an inbox item.
- Use the fixture module and local image site only (no internet).

Out: live-site checks (the nightly smoke run covers those).

## Seams under test
The browser, through Playwright, against the real binary; the CBZ checked by unzipping the downloaded file.

## Acceptance criteria
- [ ] The job runs on every PR and push to main and fails on any step of the flow.
- [ ] Runtime under ~10 minutes.

## FMD2 references
- `docs/plan.md` Verification ("End to end")
