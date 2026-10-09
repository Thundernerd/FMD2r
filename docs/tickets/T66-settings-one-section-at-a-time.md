# T66: Show one Settings section at a time
Deps: none

## Goal
The Settings page renders every section in one long column (`web/src/routes/settings/+page.svelte:240-283`): the 12 sections from `SETTINGS_SECTIONS` (`web/src/lib/settings/sections.ts`), then "Website modules" and "Accounts". The table of contents only scrolls to a section (`jump`, :165-168) and highlights the one being read with an `IntersectionObserver` (:170-189). Clicking a section title should show that section alone.

## Scope (in/out)
In:
- Only the selected section is shown. Clicking a TOC entry (or picking it in the narrow-screen "Jump to section" select) switches to it; the active entry keeps `aria-current`.
- The selected section is in the URL as `#section-<id>`, so a link opens it and Back/Forward move between sections. The existing `/settings#section-server` link (`web/src/lib/components/OpenServerBanner.svelte:16`) opens the Server section. No hash, or an unknown one, opens the first section (General). `?module=` keeps working beside it and opens "Website modules".
- Drop the scroll-spy `IntersectionObserver`; the active entry is the selected section.
- Unsaved edits survive switching sections; the save bar still saves and discards all of them. A TOC entry with unsaved changes or an error is marked (e.g. a dot), since its fields are now out of view.
- When a save fails validation, the page switches to the section holding the first invalid field before scrolling to it (today's `.field.invalid` lookup, :156-160, finds nothing in a hidden section).

Out: new settings, reordering sections, a search box.

## Seams under test
- Web component/route test: with `#section-output`, only the Output section is rendered; clicking "Server" shows only Server and sets the hash.
- An edit in General, then a switch to Output, keeps the save bar's "Unsaved changes" and the General TOC entry is marked; Save sends the General change.
- A save rejected on a field in another section switches to that section and focuses the field.
- Playwright (`web/e2e/settings.test.ts`): update the existing "Jump to section" tests (:65-76) to the new behaviour; `/settings#section-server` opens Server.

## Acceptance criteria
- [ ] Clicking a section title shows that section only, and the URL links to it.
- [ ] No unsaved change or validation error is lost or hidden by switching sections.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None (web UI only).
