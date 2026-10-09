# T75: Sort Settings → Accounts like the website lists
Deps: none

## Goal
Settings → Accounts (`web/src/lib/components/settings/AccountsPanel.svelte`) lists accounts in the order `GET /api/accounts` returns them: by module ID (`crates/fmd-server/src/accounts.rs:117`), and module IDs are hashes, so the order looks random. T72 fixed the same problem for Settings → Website modules: sorted by name ignoring case, then by host for repeated names, grouped by category, using the shared helpers in `web/src/lib/modules.ts` (`groupModules`, `moduleLabel`, `repeatedNames`). Accounts should follow the same order.

## Scope (in/out)
In:
- Accounts are listed by name, ignoring case, under category headings, in the same order as the Website modules list. Two accounts for modules with the same name show the host, as `moduleLabel` does.
- Reuse the shared helpers rather than sorting again. `AccountInfo` has no category or root URL, so join each account with its module's `ModuleSummary` from `GET /api/modules` (or add those fields to `AccountInfo`, updating `openapi.json` and the web types).
- An account whose module isn't in the summaries (e.g. still loading) is listed under "Other", not dropped.

Out: changing what an account row shows or does; search in the Accounts list.

## Seams under test
- Web component test (`AccountsPanel`): accounts given in module-ID order render in name order under their category headings; two modules with the same name are ordered by host and labelled with it.

## Acceptance criteria
- [ ] Settings → Accounts is sorted and grouped like Settings → Website modules.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None.
