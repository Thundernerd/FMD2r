# Usage counts across upstream `lua/`

The corpus is upstream FMD2 `ad3a5b63`, `lua/**/*.lua`: 676 files. Of those, 624 are in `modules/`, 41 in `templates/`, 7 in `utils/`, 4 in `websitebypass/` and 1 in `extras/`. All 676 compile with `luac5.4 -p`.

## Method

* **Global reads and writes.** For each file, `luac5.4 -p -l -l` lists the bytecode. The script counts `GETTABUP`/`SETTABUP` instructions on `_ENV` with a constant key. Each count is an exact number of global accesses at bytecode level, so comments and strings are excluded.
* **Member accesses** (`x.Name` or `x:Name`). The script strips comments (`--[[…]]` and `--…`), then counts the regex `[.:]Name(?![A-Za-z0-9_])` and reports dot and colon forms separately.
  * Host names are PascalCase and Lua stdlib names are lower case, so collisions are rare.
  * Names that several host classes share (`Get`, `Count`, `ToString`, `Add`, `Clear`) are also counted with a receiver-specific regex where that matters, for example `(PageLinks|ChapterLinks|…|NAMES|LINKS|Headers|Cookies)\.Add`. Each table states which count it shows.
* **`require`.** The script counts the regex `require\s*\(?\s*['"]([^'"]+)['"]` after stripping comments.
* **Scripts.** The analysis lived in session scratch scripts (`globals.py`, `usage.py`). Rerunning the method above on a newer upstream commit reproduces the counts.

## 1. Calling-convention check

**Colon calls on host members: 0.** That holds for every bound method name in every object and library. Dot calls are universal ([runtime.md §5.2](runtime.md#52-method-calls-upvalue-bound-self-dot-syntax)).

## 2. Global reads

These are the globals that are read but never assigned in any `lua/` file and are not Lua stdlib. In other words, they are the host-provided globals plus module bugs.

| Global | Reads | Files | Note |
|---|---:|---:|---|
| `MANGAINFO` | 3337 | 334 | host |
| `HTTP` | 3152 | 368 | host |
| `no_error` | 1876 | 664 | host constant |
| `MODULE` | 1631 | 333 | host |
| `CreateTXQuery` | 1159 | 347 | host |
| `net_problem` | 915 | 324 | host constant |
| `MaybeFillHost` | 647 | 277 | host |
| `NewWebsiteModule` | 623 | 623 | host (Init only) |
| `TASK` | 583 | 308 | host |
| `MangaInfoStatusIfPos` | 220 | 218 | host |
| `UPDATELIST` | 92 | 72 | host |
| `GetBetween` | 75 | 44 | host |
| `WORKID` | 52 | 24 | host |
| `Trim` | 21 | 16 | host |
| `sleep` | 15 | 12 | host |
| `asUnknown` / `asValid` / `asChecking` / `asInvalid` | 15 / 13 / 12 / 12 | 12 | host constants |
| `SeparateRight` | 10 | 5 | host |
| `MANGACHECK` | 5 | 1 | host (Check Modules only) |
| `SeparateLeft` | 5 | 4 | host |
| `net_error` | 2 | 2 | **bug**: nil |
| `NextJs` | 2 | 1 | never assigned as a global: nil |
| `FILENAME` | 1 | 1 | host |
| `StringUnscramble`, `WorkId`, `Module`, `tointeger`, `langs`, `splitstr`, `bit32_band`, `lshift` | 1 each | 1 each | **bugs or dead paths**: nil |

Host globals that are also *written* by modules:

| Global | Uses |
|---|---|
| `URL` | 927 reads, 1 write |
| `NAMES` | 301 reads, 1 write |
| `LINKS` | 301 reads, 1 write |
| `PAGENUMBER` | 8 reads, 161 writes |
| `LOGGER` | 12 reads, 1 write (set by `websitebypass.lua`) |

`Init` is written in 625 files. `print` is read 51 times in 14 files.

Host globals with no reads: `information_not_found`, `WORKPTR`, `PATH`.

Stdlib globals read: `require` (637 / 493), `os` (49 / 28), `io` (13 / 5), `debug` (1 / 1). `load`, `loadfile`, `dofile`, `collectgarbage`, `package`, `utf8` (as a global), `unpack`, `setfenv`, `getfenv`, `loadstring`, `module`, `bit32` and `jit` are never read.

## 3. `require` targets

| Target | Occurrences | Files |
|---|---:|---:|
| `templates.Madara` | 125 | 125 |
| `fmd.crypto` | 84 | 64 |
| `fmd.env` | 70 | 70 |
| `templates.MangaThemesia` | 70 | 70 |
| `utils.json` | 49 | 39 |
| `templates.MangaReaderOnline` | 19 | 19 |
| `templates.KeyoApp` | 14 | 14 |
| `templates.MangaHub` | 11 | 11 |
| `templates.VTheme` | 10 | 10 |
| `fmd.duktape` | 9 | 8 |
| `templates.Genkan`, `templates.WPComics` | 9 each | 9 each |
| `templates.GroupLe`, `templates.Liliana`, `fmd.imagepuzzle` | 8 | 8 / 8 / 7 |
| `templates.MangaEsp`, `FoOlSlide`, `NovelsHub`, `NiAdd`, `NineManga`, `NovelCool` | 7 each | 7 each |
| `templates.FMReader` | 6 | 6 |
| `utils.nodejs` / `utils.nextjs` | 5 / 5 | 3 / 5 |
| `templates.MadTheme`, `KiruBase` | 4 | 4 |
| `fmd.fileutil` | 3 | 2 |
| `templates.ZManga`, `MultiChan`, `LibGroup`, `FuzzyDoodle`, `Hiper`, `ColorlibAnime`, `MangaK`; `utils.jsunpack` | 3 | 3 |
| `templates.MangAdventure`, `DataLife`, `Guya`, `Roseveil`, `EZManga`, `ZeroTheme`, `SchaleNetwork`, `HeanCMS`, `MangaBox`, `MangaTaro`, `SPA`; `utils.sha256`; `fmd.subprocess`; `fmd.mangafoxwatermark` | 2 | 1–2 |
| `templates.GoDa`, `SinMH`, `MangaCatalog`, `SpicyTheme`; `utils.lzstring`, `utils.protoc`; `fmd.gzip`, `fmd.logger`; `websitebypass.cloudflare`, `websitebypass.ddos-guard`; **`pb`** (native C module); `utf8` | 1 | 1 |
| `fmd.pcre2`, `fmd.strings` | **0** | 0 |

The regex also matched `require(...)` inside JavaScript text in `utils/nodejs.lua` and in the Madara and MangaGo JS snippets: `puppeteer`, `vm`, `http`, `fs`, `utils/crypto-js.min.js`, `utils/cryptojs-aes-format.js`. Those are Node.js or Duktape requires, not Lua ones. `utils/protoc.lua` also uses `pcall(require, "pb")`.

## 4. Hook registrations (Init writes `m.OnX = …`)

| Hook | Files | | Hook | Files |
|---|---:|---|---|---:|
| `OnGetInfo` | 623 | | `OnDownloadImage` | 12 |
| `OnGetPageNumber` | 623 | | `OnTaskStart` | 3 |
| `OnGetNameAndLink` | 617 | | `OnAccountState` | 2 |
| `OnGetDirectoryPageNumber` | 235 | | `OnBeforeUpdateList` | 1 |
| `OnBeforeDownloadImage` | 114 | | `OnAfterImageSaved` | 1 |
| `OnGetImageURL` | 40 | | `OnCheckSite` | 1 |
| `OnLogin` | 28 | | `OnAfterUpdateList`, `OnSaveImage` | **0** |

Other Init writes:

| Property | Files |
|---|---:|
| `ID`, `Name`, `RootURL` | 623 each |
| `Category` | 619 |
| `SortedList` | 128 |
| `TotalDirectory` | 38 |
| `AccountSupport` | 29 |
| `LastUpdated` | 28 |
| `MaxTaskLimit` | 18 |
| `MaxConnectionLimit` | 18 |
| `MaxThreadPerTaskLimit` | 1 |
| `DynamicPageLink` | 1 |

## 5. `Storage` keys

The keys used are `URL` (variable), `listtype`, `Auth`, `UserHash`, `lastDelay`, `last_delay`, `token`, a variable `mid`, `puppeteer_ua`, `puppeteer_cookies`, `chaptertype`, `req_times`, `reload`, `<prefix>token`, `<prefix>sign`, `<prefix>expiry`, `Node`, `<mid>_time`, `madokamiulist`, `<key>_time`, `id`, `gg`, `exp`, `checksum`, `/<link>` and `fullpageload`.

None collides with a `Storage` member name. Variable keys built from URLs could contain `=`, which `TStringList.Values` cannot store.

## 6. Load-bearing vs unused (summary)

**Load-bearing.** Every module or template depends on these:

* `HTTP`: `GET`, `POST`, `Reset`, `Document.ToString`, `Headers.Values`, `Cookies.Values`, `MimeType`, `UserAgent`, `ResultCode`, `Terminated`.
* `CreateTXQuery` and the XPath methods.
* `IXQValue`: `Get()` iteration and `Get(i)`, `GetAttribute`, `GetProperty`, `ToString`, `Count`.
* TStrings: `Add`, `Reverse`, `Values`, `Count`, `Clear`, `[i]` read and write.
* `MANGAINFO` fields, `TASK.PageLinks` / `PageNumber` / `PageContainerLinks` / `ChapterLinks` / `CurrentDownloadChapterPtr`.
* `MODULE.RootURL` / `Storage` / `GetOption` / `Account.*` / `CurrentDirectoryIndex`.
* `UPDATELIST.CurrentDirectoryPageNumber` / `UpdateStatusText`.
* `MaybeFillHost`, `MangaInfoStatusIfPos`, `GetBetween`.
* `fmd.env.SelectedLanguage`, the common `fmd.crypto` functions, `fmd.duktape.ExecJS`.
* All `require` paths into `templates.*` and `utils.*`.

**Low use (1–5 files), still needed for drop-in:**

* `HTTP.Request` / `XHR` / `LastURL` / `EnabledCookies` / `ParseServerCookies` / `ClearCookiesStorage`.
* `MODULE.ClearCookies` / `AddServerCookies` / `RemoveCookies`; `TASK.FileNames` / `CurrentMaxFileNameLength`.
* `fmd.gzip`, `fmd.fileutil`, `fmd.subprocess.RunCommandHide`, `fmd.logger`, `fmd.mangafoxwatermark`, `fmd.imagepuzzle`.
* The rare crypto functions: X25519, SecretStream, GCM, CTR.
* `MANGACHECK`, `OnCheckSite`, `OnTaskStart`, `OnAccountState`, `OnBeforeUpdateList`, `OnAfterImageSaved`.
* The native `pb` module.

**Unused at the baseline.** Implement these cheaply, or let them fail loudly per map #1, because future upstream modules may adopt them:

* `fmd.pcre2` and `fmd.strings`.
* `HTTP.HEAD`, `ResetBasic`, `GetCookies`, `SetProxy`, `ResultString`, `AddServerCookies`, `ClearCookies`.
* `MODULE.GetServerCookies`, `Guardian`, `Tag`, `ActiveConnectionCount`, `InformationAvailable`, `FavoriteAvailable`; the `OnAfterUpdateList` and `OnSaveImage` hooks.
* `TCriticalSection` methods.
* `Storage.Text` / `Remove` / `Free` / `Destroy` / `Tag` / `Enable` / `Status`.
* TStrings `Get`/`Set`/`Strings`/`IndexOf`/`IndexOfName`/`Sort`/`AddText`/`DelimitedText`/`Delimiter`/`NameValueSeparator`/`LoadFrom*`/`SaveToStream`.
* MemoryStream `ReadString`/`LoadFromFile`/`SaveToFile`/`Size`.
* IXQValue `InnerHTML`/`OuterHTML`/`InnerText`.
* `ImagePuzzle.Flips`/`HorBlock`/`VerBlock`.
* `fmd.env.Directory`/`ExeName`/`Version`; `fmd.fileutil.ExtractFileName`; `fmd.subprocess.RunCommand`/`New`; `fmd.logger.Send`.
* Most of `fmd.crypto` (see [globals-and-libraries.md](globals-and-libraries.md#fmdcrypto-luacryptopas-basecryptopas-synapse-synacodepas)).
* The globals `information_not_found`, `WORKPTR`, `PATH`.

## 7. Upstream module mistakes a port must tolerate

* Writes to unbound properties: `HTTP.FollowRedirection` (`websitebypass/cloudflare.lua:134,140,250,255`) and `MANGAINFO.Artist` (`modules/OrckuMangas.lua:91`). These must stay silent no-ops.
* Reads of undefined globals (see [§2](#2-global-reads)). These must stay `nil`, not errors.
* `MODULE.AddServerCookies(cookies)` without a URL (`Madokami.lua:75`, `SoftKomik.lua:132`) stores domain-less cookies that never match. A port that "fixes" this would change request behaviour.
* `websitebypass/ddos-guard.lua` calls `self:sleepOrBreak`, which only `cloudflare.lua` defines. That path raises a Lua error on retry.
* Windows-specific paths in Lua:
  * `fmd.LuaDirectory .. 'extras\\mangafoxtemplate'` (`FanFox.lua:36`);
  * `lua\websitebypass\websitebypass_config.json` (`websitebypass/cloudflare.lua`);
  * `cmd.exe` in `utils/nodejs.lua`.

  These need a Windows-path shim or are platform casualties. That decision belongs to the anti-bot (#6) and native-deps (#9) tickets.
