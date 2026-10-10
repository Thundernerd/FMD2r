# T80: Setup step: download format
Deps: T78

## Goal
Chapters are saved as a folder of images, ZIP, CBZ, PDF or EPUB (`output.format`, "Save chapters as" in Settings → Output, `web/src/lib/settings/sections.ts:155`). A new user should choose it during setup (T78), knowing what each format is for.

## Scope (in/out)
In:
- A "Download format" step with the five formats as large choices, each with a one-line description of when it fits (e.g. CBZ for comic readers such as Komga, Kavita or a tablet app; a folder of images for browsing files; EPUB for e-readers; PDF for anything that opens PDFs). The current value is preselected (a folder of images on a fresh install, `crates/fmd-core/src/settings/model.rs:304`).
- The format labels and values come from the same place as the Settings field, so the two can't drift.
- Next saves `output.format`. Other output and image options stay in Settings, which the step links to.

Out: the other output options (compression, image conversion) in the wizard; per-destination formats.

## Seams under test
- Web component test: the step preselects the current format; picking EPUB and pressing Next saves `output.format: "epub"`.
- Playwright (mocked API): pick a format during setup, then Settings → Output shows it.

## Acceptance criteria
- [ ] Setup lets the user choose the download format, with a short explanation of each.
- [ ] The web checks, unit tests and e2e tests pass.

## FMD2 references
None.
