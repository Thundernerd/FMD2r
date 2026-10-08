# Can upstream FMD2 run headless for differential testing?

Research for [#7](https://github.com/Thundernerd/FMD2r/issues/7), part of map [#1](https://github.com/Thundernerd/FMD2r/issues/1).
Source: upstream checkout `~/Repositories/Forks/FMD2` at baseline `ad3a5b63` (2026-10-04). Paths below are relative to that checkout unless marked otherwise.

## Answer

**No, not as shipped.** FMD2 is a GUI-only Lazarus/LCL application with no headless or scripting entry point. `--lua-dofile` is a developer flag that turns off the bytecode cache. It does not run a script. `--dump-loaded-modules` writes module IDs and names to the log file, and that's all it does. Module hooks (`GetInfo`, `GetPageNumber`, `GetNameAndLink`) only run when the GUI triggers them. There is no upstream CI or test harness.

FMD2 **does run on Linux under Wine** (upstream users confirm it; a community Docker image runs it under Wine + Xvfb). A **native Linux build is not available and would not compile from the tree as-is**.

**Most practical route:** don't make FMD2 the oracle for automated runs. Capture **golden fixtures** once, either by driving FMD2 under Wine by hand or by reading the SQLite files it writes. Pin them to recorded HTTP responses, then replay them against FMD2r. Building a real headless harness from the Pascal sources is possible but expensive (see "Option C").

## Findings

### 1. Command-line flags (`mangadownloader/md.lpr`)

The comment block at the top of `begin` documents three flags. The parse loop handles more:

| Flag | Effect | Source |
|---|---|---|
| `--lua-dofile` | Sets `AlwaysLoadLuaFromFile := True` | `md.lpr` lines ~84-88 |
| `--dump-loaded-modules` | After module scan, calls `DumpLoadedModules` | `forms/frmMain.pas:2051` |
| `--dorestart-pid=N` | Windows only: wait for the old process when restarting | `md.lpr` `{$ifdef windows}` block |
| `--no-commit-queue`, `--max-commit-queue`, `--max-flush-queue`, `--max-big-flush-queue`, `--backup-interval` | SQLite queue and backup tuning | `md.lpr` parse loop |

After parsing, `md.lpr` always does `Application.Initialize; Application.CreateForm(TMainForm, MainForm); ... Application.Run`. No code path exits early or skips the main form.

**What `--lua-dofile` actually does.** `AlwaysLoadLuaFromFile` is read in three places:
- `baseunits/lua/LuaBase.pas:221` (`LuaLoadFromStreamOrFile`) loads with `luaL_loadfile` from disk instead of from the cached bytecode stream.
- `baseunits/lua/LuaWebsiteModules.pas:716` (`ByteCode`) returns `nil`, so no bytecode cache is built.
- `baseunits/lua/LuaHandler.pas:103` drops the chunk from `FLoadedChunks` so `require`d files reload each time.

The only other effect is that `frmMain.pas:1420` appends ` --lua-dofile` to the window caption. It is also on by default in `DEVBUILD` builds (`LuaBase.pas:29`; `-dDEVBUILD` is set only for the "Debug Leaks" modes in `md.lpi:290`).

**So `--lua-dofile` gives a script nothing.** No user-supplied script runs. You edit module files and FMD2 re-reads them on the next hook call without a restart. That's a module-author convenience. `HTTP`, `MANGAINFO`, `TASK` and the rest are only available inside hook calls the engine makes (next section).

**`--dump-loaded-modules`** (`frmMain.pas:2030-2039`) sends `loaded modules: N` plus `"<ID> <Name>"` lines to `Logger`. The output only reaches disk if logging is enabled in `userdata/settings.json` (`[logger] Enabled`, read in `md.lpr`). It runs inside `TMainForm.tmStartupTimer`, so the GUI has to start first.

### 2. Where hook globals come from (`baseunits/lua/LuaWebsiteModules.pas`)

Each hook wrapper builds a Lua state, binds Pascal objects as globals, then calls the module function:

| Hook | Globals pushed | Lines |
|---|---|---|
| `DoGetDirectoryPageNumber` | `HTTP`, `UPDATELIST` | 190-204 |
| `DoGetNameAndLink` | `HTTP`, `NAMES`, `LINKS`, `UPDATELIST`, `URL` | 219-235 |
| `DoGetInfo` | `MANGAINFO`, `HTTP`, `URL` | 245-257 |
| `DoGetPageNumber` | `TASK`, `HTTP`, `URL` | 285-296 |
| image/download hooks | `TASK`, `HTTP`, `URL` | 315-381 |
| all | `MODULE` | 822 |

This matches `docs/LUA-REFERENCE.md:291-293`. The Pascal objects behind these globals (`TMangaInformation`, `TTaskThread`, `THTTPSendThread`) belong to the download manager and GUI threads. Nothing exposes them to an outside caller.

### 3. Other ways to drive it: none

- **IPC:** `md.lpr` uses `TSimpleIPCClient` only for single-instance detection. The server handler `TMainForm.FMDInstanceReceiveMsg` (`frmMain.pas:2912`) just shows "already running" and brings the window to the front. It takes no commands.
- **Coupling:** the engine units depend on the GUI. `uDownloadsManager.pas`, `uBaseUnit.pas` and `FMDVars.pas` all `uses frmMain`, and `uBaseUnit` pulls in `VirtualTrees` and `Graphics`. `FMDOptions.pas` uses `Forms`. A console harness can't just link `LuaWebsiteModules` without stubbing or untangling `frmMain`.

### 4. Linux: native build vs Wine

**Native Lazarus/FPC Linux build is not ready:**
- `md.lpi` defines six build modes, all `win32` or `win64` (`md.lpi:32-296`). Post-build steps use `cmd.exe /c copy` (`md.lpi:230,282`). Releases are made by `make_release_win.bat`.
- Partial `{$ifdef UNIX}` support exists: `cthreads`; `UTF8Process` vs `ShellApi` in `uMisc.pas` and `uBaseUnit.pas`; `WinAPI` is only used under `{$ifdef windows}`. Linux library names are declared: `liblua5.4.so` (`lua54.pas:111`), `libpcre2.so` (`pcre2lib.pas:62`), `libwebp.so` (`webp.pas:12`), `libzstd.so` / `libbrotlidec.so` (`ZstdDec.pas`, `BrotliDec.pas`), and `libduktape_linux64.so` (`Duktape.Api.pas:38`).
- Blockers:
  - `frmMain.pas:18` uses `FakeActiveX` on non-Windows, and no such unit exists in the repo.
  - `frmMain` unconditionally uses `uWin32WidgetSetDark`, and `md.lpr` uses `uDarkStyle`/`uMetaDarkStyle` (from the external MetaDarkStyle package, built for Windows dark mode).
  - Several required packages are third-party (InternetTools, MetaDarkStyle, Lazarus_CustomControls, VirtualTreeView V5, RichMemo, MultiLog, DCPCrypt; `README.md:44-73`). The `3rd/internettools` submodule (`.gitmodules`) is not checked out locally.
  - `pb.dll` (lua-protobuf) ships only as a Windows DLL (`dist/readme.md`).
- The maintainer's own position ([dazedcat19/FMD2#64](https://github.com/dazedcat19/FMD2/issues/64)): "Just try to compile it with lazarus with linux. I already put some defines for windows specific. But some other might need adjustment." Nobody has done it upstream.

**Wine works.** Users report it running fine under Wine and Proton ([#64](https://github.com/dazedcat19/FMD2/issues/64), [#839](https://github.com/dazedcat19/FMD2/issues/839)). There is one Wine-specific module bug, [#638](https://github.com/dazedcat19/FMD2/issues/638) "Mangadex Mapping Issue Under Wine". A community image, [Banh-Canh/docker-FMD2](https://github.com/Banh-Canh/docker-FMD2) (Wine + Xvfb + noVNC), shows it runs with a virtual display. That's a secondary source and I didn't verify it. It is still a GUI: it needs an X display, and someone has to click.

**Released binary vs baseline (a surprise):** the latest upstream release is **2.0.34.5 (2025-03-17)**, Windows only (`fmd_2.0.34.5_{i386-win32,x86_64-win64}.7z`; `latest_version.json`, `changelog.txt`). `md.lpi` at `ad3a5b63` still says 2.0.34.5. But there are **23 commits to `baseunits`/`mangadownloader` since that release**, several of which add Host API: `ImagePuzzle` flip (#3252), more crypto algorithms (#3226, #2945), AES-CTR (#3044), `LuaGZip` exposed to Lua (#3175), and a Module Check Integrity tab (#2685). FMD2 pulls the latest Lua modules from `master` (`dist/config.json` → `GitHub.ref: master`). So **the official binary can be older than the Host API that current upstream modules use**. An oracle based on the release `.exe` would fail some modules for reasons that have nothing to do with FMD2r. A faithful oracle at `ad3a5b63` needs a **source build**, and the only supported toolchain is Windows Lazarus.

### 5. Upstream CI / tests: none

- `.github/` contains only `ISSUE_TEMPLATE`. `gh api repos/dazedcat19/FMD2/actions/workflows` returns `total_count: 0`.
- No test directories or test units outside module code. `scripts/` holds Windows `.bat` helpers (module conversion, submodule install, DB rename).
- The nearest thing is the in-app **Module Check Integrity** tab (`frmCheckModules`, created in `md.lpr`; added in #2685). It's GUI-driven too.

### 6. Local toolchain (checked with `which` / `pacman -Q`)

- `wine` 11.18 and `winetricks`: installed.
- `lazbuild`, `fpc`, `ppcx64`: **not installed**.
- `Xvfb` / `xvfb-run`: **not installed**. Running FMD2 under Wine would open a window on the desktop.
- `7z` / `7za`: installed, so the release `.7z` can be unpacked.
- No local `fmd.exe` and no `bin/` build output. I didn't download or launch FMD2.

## Options for producing expected outputs

| Option | What it takes | Fidelity | Automation | Verdict |
|---|---|---|---|---|
| **A. Manual golden capture under Wine** | Release `.7z` + Wine (+ Xvfb/VNC in a container). Run "update list" / "get info" / add a download by hand, then read results from SQLite: `data/<website>.db` (names/links from `GetNameAndLink`) and `userdata/downloads.db` (`chapterslinks`, `chaptersnames`, `pagelinks` columns; `baseunits/DownloadsDB.pas:84-91`). Manga info fields show in the GUI. I haven't checked whether they're persisted anywhere besides the chapter list. | Real FMD2, but the release binary lags the `ad3a5b63` Host API | Manual, per site | **Recommended for a small seed set** |
| **B. FMD2r self-consistency + recorded HTTP** | Record HTTP responses once (from FMD2 runs in A, or from FMD2r), freeze them, snapshot FMD2r outputs, and re-run against the frozen responses | Needs a human or an A-capture to approve the first snapshot | Fully automated in CI | **Recommended as the main regression net** |
| **C. Headless Pascal harness** | Console `.lpr` that stubs `frmMain`/GUI units, links `LuaWebsiteModules` + `httpsendthread`, calls `DoGetInfo`/`DoGetNameAndLink`/`DoGetPageNumber`, and prints JSON. Built with Windows Lazarus (natively or Lazarus-under-Wine) plus all third-party packages, then run under Wine without a display (no LCL forms). | Exact, at any commit, including `ad3a5b63` | Fully automated | Feasible but large: untangling `frmMain` uses and a Lazarus toolchain under Wine. Only worth it if A+B prove too weak. |
| **D. GUI automation (xdotool/AutoHotkey under Wine)** | Script clicks in the Wine window | Real FMD2 | Brittle | Not recommended |
| **E. Native Linux build** | Fix `FakeActiveX`, dark-mode units, packages, `pb` `.so`; port the build | Exact | Automated | Not recommended; it would turn into a port of its own |

**Recommendation:** use B as the CI mechanism, seed and spot-check it with A, and record which FMD2 version each golden file came from. Keep C on the shelf. Sites are live and change, so any oracle has to be paired with recorded HTTP to be repeatable. That limit holds whichever option we pick.

## Blockers summary

1. There is no headless/CLI mode and no command channel. Hooks run only from GUI-owned threads.
2. `--lua-dofile` is a reload-from-disk flag, not a script runner. Hook globals exist only inside engine-made hook calls.
3. The engine is coupled to `frmMain`, so building a harness means stubbing GUI units.
4. A native Linux build is blocked by the missing `FakeActiveX`, Windows-only dark-mode units, Windows-only build modes and third-party packages.
5. The latest release binary (2.0.34.5, 2025-03) predates the baseline Host API, so a faithful oracle needs a source build on a Windows Lazarus toolchain.
6. There is no upstream CI or tests to reuse.
