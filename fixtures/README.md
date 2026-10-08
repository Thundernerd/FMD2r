# Test fixtures

## `lua/`: upstream Lua snapshot

A copy of the `lua/` tree (`modules`, `templates`, `utils`, `websitebypass`, `extras`) from
[dazedcat19/FMD2](https://github.com/dazedcat19/FMD2), the repository FMD2 itself updates its
modules from. `lua/UPSTREAM_REF` holds the commit it was taken from.

The snapshot is **committed**, not fetched in CI, so `cargo test` runs offline and every run
checks the same modules. Refresh it on purpose and commit the result:

```sh
scripts/sync-upstream-lua.sh               # latest master
scripts/sync-upstream-lua.sh --ref <sha>   # a specific commit, branch or tag
```

The script replaces `fixtures/lua` wholesale; don't edit files in it by hand.

Tests reach the snapshot through the `fmd-testkit` dev crate: `corpus_root()`, `module_files()`,
`check_each()` (runs a check on every module and reports all failures at once) and
`scan_host_api_names()` (the Host API names a Lua source references).

## `smoke/`: the smoke list

`smoke/list.toml` names representative upstream modules, one `[[entry]]` each:

```toml
[[entry]]
name = "mangadex"                                # the entry's directory, smoke/<name>/
module_id = "d07c9c2425764da8ba056505f57cf40c"   # the module's ID (`m.ID`), not its name
manga_url = "https://mangadex.org/title/..."     # `fmd2r module info` runs on it
chapter_url = "https://mangadex.org/<id>"        # `fmd2r module pages` runs on it
notes = "JSON API; fmd.crypto"                   # the template/libraries/features it covers
```

The entries are picked to cover the major templates, JSON APIs, the `fmd.crypto`,
`fmd.duktape`, `fmd.imagepuzzle` and `pb` libraries, cookies and accounts, and
`DynamicPageLink`; each entry's `notes` say what it covers. A `chapter_url` is the module's
`RootURL` plus one of the chapter links `module info` printed, as FMD2 would build it.

Each entry's directory holds its recording, made with `fmd2r module info|pages --record`
(format in `docs/fixtures.md`), and snapshots of what those commands printed:

```
smoke/<name>/
  info/   info.json    the HTTP fixtures of `module info <manga_url>`, and its output
  pages/  pages.json   the same for `module pages <chapter_url>`
```

Image response bodies are dropped after recording (the exchange's `body` becomes `null`): `info`
and `pages` don't need them.

The `fmd-smoke` crate drives the list. Three places run it:

- **CI**, offline: `cargo test -p fmd2r --test smoke` replays every entry (`--replay`) and fails
  when an output differs from its snapshot. Replays run with an empty `PATH`, so a module that
  runs a program through `fmd.subprocess` (node via `utils.nodejs`, python) can't reach the
  network past the recording; such modules can't be on the list.
- **Nightly**, live (`.github/workflows/smoke-nightly.yml`): runs every entry against the live
  site and on its recording, and uploads `smoke-report` with a Markdown report (also on the run's
  summary page). An entry whose replay fails is an *FMD2r regression*; one that fails only live
  had its *site or module change*. A live step passes when `info` finds a title and chapters and
  `pages` a resolved page link. The workflow never fails on entry failures.
- **By hand**: `cargo build -p fmd2r -p fmd-smoke`, then from the repository root
  `target/debug/fmd-smoke run [--live] [NAME...]` prints results as JSON, and
  `target/debug/fmd-smoke report --live L --replay R` classifies two such files.

### Re-recording an entry

When a site changed (the nightly report lists it under "Site/module changed") or after adding an
entry to `list.toml`, record it again:

```sh
scripts/smoke-record.sh <name> [<name>...]
```

The script builds `fmd2r` and `fmd-smoke` and records the entry from the live site, replacing
its fixtures and snapshots. It fails, and writes no snapshot, when the live run finds nothing
(no title or chapters, no resolved pages) or when replaying the fresh recording prints
something else than the live run did (a module whose requests vary between runs, e.g. with a
timestamp in the URL). Review `git diff fixtures/smoke` before committing: a recording is a
copy of the site's responses and is committed as is. Some sites echo the client's IP address
back (e.g. MangaLib's DDoS-Guard `__ddg9_` cookie): find it with `grep -rF "$(curl -s
https://ifconfig.me)" fixtures/smoke` and replace it with `0.0.0.0`, then check the entry still
replays (`target/debug/fmd-smoke run <name>`).
