# T05: Global helper functions
Deps: T03

## Goal
Provide the global functions every module may call without a `require`: `print`, `sleep`, `Trim`, `MaybeFillHost`, `MangaInfoStatusIfPos`, `GetBetween`, `SeparateLeft`, `SeparateRight`, with FMD2's exact string semantics.

## Scope (in/out)
In:
- `print(...)`: routes to the FMD2r log (tracing) instead of stdout, matching FMD2's argument joining.
- `sleep(ms)`: blocks the worker thread for `ms` milliseconds (cancellable later by T14; accept a cancellation hook now).
- `Trim(s)`: FPC `Trim` semantics (strips chars `<= ' '` at both ends).
- `MaybeFillHost(host, url)`: prefix relative URLs with host, exactly as `uBaseUnit.pas` does (handle `//x`, `/x`, `x`, already-absolute).
- `MangaInfoStatusIfPos(search, ongoing='ongoing', completed='complete', hiatus='hiatus', cancelled='cancel')`: accepts 1 to 5 arguments; lowercases everything and does substring matching, where each status string may hold `|`-separated alternatives; checks ongoing, completed, hiatus, cancelled in that order and returns `'1'`, `'0'`, `'2'`, `'3'`, or `'Unknown'` (`RS_InfoStatus_Unknown`, an untranslated resource string) when nothing matches; an empty search string returns `''`.
- `GetBetween(sep1, sep2, s)`, `SeparateLeft(s, sep)`, `SeparateRight(s, sep)`: Synapse `synautil` semantics (argument order matters).
- Register them via a public `Runtime` installer used by later tickets.

Out: `CreateTXQuery` (T08); `NewWebsiteModule` (T06).

## Seams under test
Lua snippets via the `fmd-lua` runtime API, with expected values derived from the Pascal sources:
```lua
assert(Trim('  a b \t\n') == 'a b')
assert(MaybeFillHost('https://h.com', '/x/y') == 'https://h.com/x/y')
assert(MaybeFillHost('https://h.com', 'https://o.com/z') == 'https://o.com/z')
assert(MangaInfoStatusIfPos('Status: Ongoing') == '1')
assert(MangaInfoStatusIfPos('Completed') == '0')
assert(MangaInfoStatusIfPos('On Hiatus') == '2')
assert(MangaInfoStatusIfPos('Dropped', 'publishing', 'finished', 'hiatus', 'dropped|cancel') == '3')
assert(MangaInfoStatusIfPos('') == '')
assert(MangaInfoStatusIfPos('Licensed') == 'Unknown')
assert(GetBetween('[', ']', 'a[b]c') == 'b')
assert(SeparateLeft('k=v', '=') == 'k' and SeparateRight('k=v', '=') == 'v')
```

## Acceptance criteria
- [ ] Each helper matches the Pascal implementation on edge cases (empty strings, missing separators, multiple separators, leading `//`).
- [ ] `print` output lands in the log with the module/thread context when available.
- [ ] `sleep` blocks only the calling worker.
- [ ] Doc comments cite the Pascal source lines.

## FMD2 references
- `baseunits/lua/LuaBase.pas:53` (`luabase_print`), `:67` (`luabase_sleep`), `:73-92` (registration)
- `baseunits/lua/LuaBaseUnit.pas:17` (`lua_trim`), `:26` (`lua_maybefillhost`), `:32` (`lua_mangainfostatusifpos`), `:58` (registration)
- `baseunits/lua/LuaSynaUtil.pas:17` (`GetBetween`), `:23` (`SeparateLeft`), `:29` (`SeparateRight`)
- `baseunits/uBaseUnit.pas:943-960` (`MaybeFillHost`)
- `baseunits/uBaseUnit.pas:626-628` (default arguments), `:2793-2850` (`MangaInfoStatusIfPos`), `:230-233` (status codes)
- `baseunits/synapse/synautil.pas` (`GetBetween`, `SeparateLeft`, `SeparateRight`)
- `docs/LUA-REFERENCE.md:1057-1105` (utility functions as modules use them)
- `mangadownloader/forms/frmMain.pas:1010` (`RS_InfoStatus_Unknown = 'Unknown'`)
