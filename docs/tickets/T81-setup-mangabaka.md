# T81: Setup step: MangaBaka database
Deps: T78

## Goal
T73 added an optional local copy of MangaBaka's database for covers, formats, statuses and better metadata on Discover. It is downloaded only when the user asks, from Settings (`web/src/lib/components/settings/MangaBakaPanel.svelte`; `POST /api/metadata/mangabaka/download`, about 390 MB, `crates/fmd-server/src/metadata.rs:91`). Until then, Discover shows a hint. A new user should be offered it during setup (T78).

## Scope (in/out)
In:
- A "Metadata" step explaining what the database adds (covers on Discover, format and status filters, richer series details), that it is about 390 MB, that no title is sent to MangaBaka (only the download), and that it can be added or removed later in Settings.
- Two choices: download it now, or skip. Downloading reuses the panel's start action and progress (from the `job.metadata.*` events). The download runs in the background, so the user can press Next straight away; the finish step (T78) says it's still downloading, and progress stays visible in Settings.
- Skipping is a valid choice and doesn't nag later beyond the existing Discover hint.
- If the server can't download it (503 from the endpoint), the step says why and only offers Next.

Out: changing the download or matching itself (T73).

## Seams under test
- Web component test: "Download" starts the download through the API and shows progress from events; Next is allowed during the download; "Skip" starts nothing; a 503 shows the reason.
- Playwright (mocked API): choose download during setup, finish, and see the download in progress in Settings.

## Acceptance criteria
- [ ] Setup offers the MangaBaka database, downloading it in the background when chosen.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None.
