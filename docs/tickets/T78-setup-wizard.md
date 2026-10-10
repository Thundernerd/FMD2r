# T78: A setup wizard on first start
Deps: none

## Goal
A fresh install opens straight onto an empty library. Everything that makes FMD2r useful has to be found in Settings: where downloads go, their format, which websites Discover lists (none on a fresh install, `crates/fmd-core/src/settings/model.rs:50-51`, :62), and the optional MangaBaka database. Add a guided setup that a new user goes through before using the app. This ticket builds the wizard itself, with a welcome step and a finish step. The steps come in their own tickets: download folders (T79), download format (T80), MangaBaka (T81) and websites (T82).

## Scope (in/out)
In:
- **When it shows:** a setting such as `general.setup_completed`. While it is false, every page redirects to `/setup` (after the login screen when a password is set, `web/src/routes/+layout.svelte:49-50`). Finishing the wizard sets it to true.
- **Existing installs skip it:** on the first start where the setting isn't stored yet, an install that already has data (stored settings, library series, tasks or lists) is marked as set up; only a fresh install starts with it false. This is how `select_listed_websites` handles T69's upgrade (`crates/fmd-core/src/settings/service.rs:159-185`). An FMD2 import done before setup doesn't change this.
- **The wizard:** a `/setup` page with a step indicator ("Step 2 of 6" and the step names), Back and Next, and a Finish on the last step. No top navigation or queue dock while it shows. It works on a phone.
- **Steps are pluggable:** a step is its own component with a title, whether Next is allowed yet, and a save action run on Next. The wizard lists the steps in one place, so each step ticket adds one entry. The order is: welcome, download folders (T79), download format (T80), MangaBaka (T81), websites (T82), finish; a step whose ticket hasn't landed yet is simply absent. Ship the welcome step (what FMD2r is and what the setup covers) and the finish step (a summary of the choices, with links to where each can be changed in Settings).
- **Progress is kept:** each step saves its settings when the user presses Next (`PATCH /api/settings`, as Settings does), so a reload resumes at the first unfinished step with the saved values. A save error shows on the step and keeps the user there.
- **Running it again:** a "Run setup again" button in Settings → General opens the wizard with the current values. Running it again doesn't redirect other pages.

Out: the step contents (T79–T82); a password or listen-address step; importing from FMD2 inside the wizard.

## Seams under test
- `fmd-core`: on the first start with the setting unstored, an install with stored data is marked set up and a fresh one isn't; a stored value is never overwritten.
- `fmd-server`: `GET /api/settings` reports the setting, and `PATCH` sets it.
- Web component tests: the wizard's Back/Next/Finish with two fake steps; Next is disabled while a step says it isn't ready; a failing save keeps the user on the step and shows the error; a reload resumes at the right step.
- Playwright (mocked API): a fresh install is redirected to `/setup` from `/` and `/discover`, finishing lands on the library, and later visits aren't redirected; an existing install never sees it.

## Acceptance criteria
- [ ] A fresh install goes through the setup before anything else; existing installs don't.
- [ ] The wizard can be run again from Settings.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace`, and the web checks, unit tests and e2e tests pass.

## FMD2 references
None (FMD2 has no first-run wizard).
