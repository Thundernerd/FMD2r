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

Every run starts in a fresh temporary working directory, removed afterwards: `utils.nodejs`
installs puppeteer and writes its scripts under `lua/utils/npm` there, as FMD2 does under its
own directory. Recording a node module needs `node` and `npm` on `PATH`.

Image response bodies are dropped after recording (the exchange's `body` becomes `null`): `info`
and `pages` don't need them.

The `fmd-smoke` crate drives the list. Three places run it:

- **CI**, offline: `cargo test -p fmd2r --test smoke` replays every entry (`--replay`) and fails
  when an output differs from its snapshot. The processes a module runs through `fmd.subprocess`
  (node via `utils.nodejs`, e.g. `comix`) are replayed from the recording too
  (`subprocess.json`, see `docs/fixtures.md`), and replays run with an empty `PATH`, so nothing
  reaches the network past the recording.
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
replays (`target/debug/fmd-smoke run <name>`). Then rebuild the XPath corpus (below).

## `xpath-corpus/`: the XPath differential corpus

Every XPath expression and CSS selector the smoke list's modules evaluate during the replay, with
the documents they ran against: the evidence that the `native` XPath backend gives FMD2's own
engine's (`fpc`) results on real module traffic (T35).

```
xpath-corpus/
  entries.jsonl         one evaluation per line, each once
  documents/<hash>.html each document body once, named by its 64-bit FNV-1a hash in hex
```

An entry names its document and expression, `css: true` for a selector, and, when it ran
against a value of an earlier evaluation (`x.XPath(expr, v)`), that value's `context`: the
evaluation it came from and the `path` of `item`s (1-based) and `property`s taken from it.

```json
{"document":"62269cfe15173811","expression":"a/@href","context":{"document":"62269cfe15173811","expression":"//li","path":[{"item":2}]}}
```

`fmd2r module info|pages --xpath-corpus DIR` records into a corpus (adding to what is there);
`fmd-smoke run --xpath-corpus DIR` passes it on to every run. `fmd2r xpath diff [--corpus DIR]`
evaluates every entry on both backends and reports the entries whose results differ (item count
and kinds, string values, serialized nodes) as Markdown, grouped by expression feature; it needs
a build with both backends (`--features xpath-fpc`). `cargo test -p fmd-xpath --features diff`
runs the same comparison on the committed corpus in CI. The nightly smoke workflow records a
corpus from its live and replay runs and diffs it too.

Rebuild the corpus after re-recording smoke entries, and commit it with them:

```sh
scripts/xpath-corpus.sh
```

`fmd2r xpath eval [--backend fpc|native] [--css] FILE EXPR` prints one evaluation in the same
normalized form, for debugging a mismatch.

## `fmd2/`: files a real FMD2 binary wrote

Ciphertext from FMD2 itself, so `EncryptString`/`DecryptString` (baseunits/uBaseUnit.pas:1556-1589)
and the importer's account and proxy-password decryption are checked against FMD2, not against a
reproduction of DCPcrypt's recipe (T43).

```
fmd2/
  encrypt_string.tsv  `plaintext hex <TAB> ciphertext`, one EncryptString call per line
  vectors.lua         the throwaway Lua module that produced encrypt_string.tsv
  userdata/           an FMD2 userdata directory: modules.json, settings.json and the
                      (empty) downloads.db, favorites.db and downloadedchapters.db
```

`crates/fmd-lua/tests/crypto.rs` checks `encrypt_string`/`decrypt_string` against
`encrypt_string.tsv`; `crates/fmd-import/tests/real_fmd2.rs` runs `fmd_import::import()` on
`userdata/`. None of the credentials are real. There is no `accounts.db`: FMD2 keeps accounts in
`modules.json`, and only names `ACCOUNTS_FILE` (baseunits/FMDOptions.pas:289) in its backup list
(mangadownloader/forms/uBackupSettings.pas:69), so it never wrote one.

### How they were made

FMD2 **2.0.34.5**, the `fmd_2.0.34.5_x86_64-win64.7z` release asset of
[dazedcat19/FMD2](https://github.com/dazedcat19/FMD2/releases/tag/2.0.34.5) (SHA-256
`dd35300ee22ef04fd56523ba241fd972fada5c780ed8b6ad4d06fd83bf231434`), run under Wine 11.18 in a
fresh prefix with a fresh `userdata` directory. FMD2 has no headless mode, and its account and
proxy fields are only set from the GUI, so the files were made in two runs instead of by typing
into the GUI:

1. Put `lua/templates`, `lua/utils` and `lua/modules/{ComX,MangaDex}.lua` from `fixtures/lua`,
   plus `vectors.lua`, into the release's `lua/` directory, and a `userdata/settings.json` with
   `{"update":{"AutoCheckLatestVersion":false},"dialogs":{"ShowQuitDialog":false}}` (no update
   check, no quit dialog). Start `fmd.exe`. `vectors.lua`'s `Init` writes
   `userdata/vectors.tsv`: FMD2's `crypto.EncryptString` of each plaintext (which calls
   `EncryptString` in uBaseUnit.pas) and `DecryptString` of the result. Close FMD2 with
   `wine taskkill /IM fmd.exe` (a close message, so `FormClose` saves `modules.json` and
   `settings.json`). `encrypt_string.tsv` is that file without the round-trip column.
2. Remove `vectors.lua`. In the saved files, set Com-X's account (enabled, `asValid`,
   `Username`/`Password` = the FMD2 ciphertexts of `fixture-user@example.test` and
   `not-a-real-password`, `Cookies` = FMD2r's ciphertext of `sid=1\0tail`), a few module
   settings and options, and in `settings.json` the proxy (`UseProxy`, `ProxyType` `HTTP`, host,
   port, `User`/`Pass` = the FMD2 ciphertexts of `proxy-user` and `pr0xy päss €`) and
   `connections/NumberOfTasks` 3. Start FMD2 again and close it the same way: it decrypts the
   credentials on load (`TWebsiteModules.LoadFromFile`, baseunits/WebsiteModules.pas:609-611;
   `LoadOptions`, mangadownloader/forms/frmMain.pas:5878-5879) and encrypts them again on save
   (WebsiteModules.pas:671-673; frmMain.pas:6074-6075). `userdata/` is what it wrote.

What the runs showed besides the vectors:

- FMD2 wrote every ciphertext back byte for byte, including the NUL cookie, so its
  `DecryptString` and `EncryptString` don't stop at a NUL. (2.0.34.5's Lua binding does: it
  passed `luaToString`/`lua_pushstring`, so `crypto.EncryptString('nul\0inside')` encrypted
  `nul`. Current FMD2 uses length-aware strings, baseunits/lua/LuaCrypto.pas:15-30, as FMD2r
  does, so that row is left out of `encrypt_string.tsv`.)
- A `ProxyType` of `SOCKS5` came back as `""`; `HTTP`, the combo box's design-time text
  (frmMain.lfm:3566), survives. Setting `cbOptionProxyType.Text` (frmMain.pas:5875) on a
  `csDropDownList` combo box at that point doesn't select the item, under Wine at least, and
  FMD2 then treats `""` as no proxy (baseunits/httpsendthread.pas:853-873).
