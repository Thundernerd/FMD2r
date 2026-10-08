# T06: Module loader, package searcher, `fmd.env` and the `MODULE` object
Deps: T03, T04

## Goal
Load the upstream website modules exactly as FMD2 does: scan `lua/modules/*.lua`, run each file's `Init()` with `NewWebsiteModule` available, capture every module it creates into a Rust `ModuleDef` registry, and expose the `MODULE` object with its properties, options, cookies, `Storage`, `Guardian` and `Account`. After this ticket, "every upstream module passes `Init`" is a testable statement.

## Scope (in/out)
In:
- **Package searcher:** install a custom `package.searchers[1]` (shifting the standard searchers down, keeping them). `require 'fmd.<lib>'` resolves to a registered host lib (Rust function returning the lib table); any other name `a.b.c` resolves to `<lua_dir>/a/b/c.lua`, loaded from a cache of compiled chunks. A missing file falls through to the standard searchers. A public `register_host_lib(name, opener)` lets later tickets add `fmd.crypto` etc.
- **`fmd.env`**: table with `Directory`, `ExeName`, `Version`, `Revision`, `LuaDirectory`, `SelectedLanguage` (FMD2r values; `LuaDirectory` points at the configured lua dir with a trailing separator).
- **Scan:** `ModuleRegistry::load_dir(lua_dir) -> LoadReport` scans `modules/*.lua` in parallel (rayon or a thread pool). Each file runs in a fresh Lua state: load and execute the chunk, then set global `NewWebsiteModule`, then call `Init()`. Errors (load, exec, missing `Init`, `Init` raising) are recorded per file in the report, not fatal.
- One file may call `NewWebsiteModule()` several times; every returned object becomes a module. Modules with empty `ID` or `Name` are dropped. `RootURL` is lowercased.
- **`ModuleDef`**: everything `Init` set on `m`: `ID`, `Name`, `RootURL`, `Category`, `MaxTaskLimit`, `MaxThreadPerTaskLimit`, `MaxConnectionLimit`, `SortedList`, `InformationAvailable`, `FavoriteAvailable`, `DynamicPageLink`, `TotalDirectory`, `AccountSupport`, `Tag`, `LastUpdated`, `CurrentDirectoryIndex`, all `On*` callback names, and the options declared via `AddOptionCheckBox`/`AddOptionEdit`/`AddOptionSpinEdit`/`AddOptionComboBox` (name, caption, kind, default, items), plus the source file path.
- **`MODULE` object** (via the T03 helper), usable both during `Init` and later in callbacks: the properties above (read/write), `ActiveTaskCount`, `ActiveConnectionCount`, `GetOption(name)` (returns the stored value, else the default, typed per option kind), `AddServerCookies`, `GetServerCookies`, `RemoveCookies`, `ClearCookies`, `Storage` (thread-safe key/value with `Values[...]`/default index, `Remove`, `Text`; shared by all threads using the module), `Guardian` (critical section with `Enter`, `Leave`, `TryEnter`; per module, shared across threads), and `Account` (present only when `AccountSupport` is true: `Enabled`, `Username`, `Password`, `Status`, `Cookies`, `Guardian`).
- Option values and cookies are read from/written to an injectable `ModuleSettingsStore` trait (in-memory impl here; T17/T18 provide the SQLite-backed one).
- Hook the T02 corpus harness: a test that loads the whole fixture corpus and reports failures. Record the current pass rate; failures caused by not-yet-implemented libs (T08–T13) are expected and listed, not hidden.

Out: running callbacks other than `Init` (T14); HTTP (T10); real persistence (T17/T18).

## Seams under test
- Lua module files written as test fixtures, loaded through `ModuleRegistry::load_dir`:
```lua
function Init()
  local m = NewWebsiteModule()
  m.ID = 'abc'; m.Name = 'Site'; m.RootURL = 'https://EXAMPLE.com'; m.Category = 'English'
  m.OnGetInfo = 'GetInfo'; m.MaxTaskLimit = 2
  m.AddOptionCheckBox('hq', 'High quality', true)
  local bad = NewWebsiteModule()   -- no ID/Name: dropped
end
```
  Assert the registry holds one `ModuleDef` with `root_url == "https://example.com"`, `on_get_info == Some("GetInfo")`, one checkbox option defaulting to true.
- `require` through the runtime API: `require 'fmd.env'.LuaDirectory`, `require 'utils.json'` resolving to `<lua_dir>/utils/json.lua`.
- `MODULE` in a later state: `MODULE.Storage['k'] = 'v'` visible from a second Lua state for the same module; `MODULE.GetOption('hq') == true`; `MODULE.Guardian.Enter(); MODULE.Guardian.Leave()`.
- Corpus test over `fixtures/lua/modules` via the T02 harness.

## Acceptance criteria
- [ ] Searcher order and fallback match `LuaPackage.pas`; `fmd.*` names never hit the filesystem.
- [ ] Load is parallel and one bad file never aborts the scan; the report names each failing file with its error.
- [ ] Multi-module files produce multiple `ModuleDef`s; empty ID/Name dropped; RootURL lowercased.
- [ ] Every `MODULE` property and method from `luaWebsiteModuleAddMetaTable` exists.
- [ ] `Storage` and `Guardian` are shared across threads for the same module and isolated between modules.
- [ ] Corpus test runs in CI and prints the pass/fail count.
- [ ] Doc comments cite the Pascal lines.

## FMD2 references
- `baseunits/lua/LuaPackage.pas:62-91` (`_findpackage`: `fmd.` prefix → host lib, else cached Lua file), `:93-115` (`RegisterLoader`: insert at searchers[1]), `:117-130` (`LoadLuaFile`: `.` → directory separator, `.lua` suffix)
- `baseunits/lua/LuaFMD.pas:15-30` (`fmd.env` contents)
- `baseunits/lua/LuaWebsiteModules.pas:467-500` (`_newwebsitemodule`, `DoInit`: exec chunk, require `Init`, register `NewWebsiteModule`, call `Init`)
- `baseunits/lua/LuaWebsiteModules.pas:502-656` (loader threads; drop empty ID/Name; lowercase RootURL; callback wiring)
- `baseunits/lua/LuaWebsiteModules.pas:713-736` (`ByteCode`: compiled chunk cached per container)
- `baseunits/lua/LuaWebsiteModules.pas:757-818` (`AddOption*`), `:848-975` (Lua wrappers: options, TotalDirectory, cookies, `GetOption`, AccountSupport)
- `baseunits/lua/LuaWebsiteModules.pas:977-989` (Account metatable), `:991-1040` (MODULE metatable: full property list)
- `baseunits/lua/LuaStringsStorage.pas:33-160` (`Storage`), `baseunits/lua/LuaCriticalSection.pas:21-55` (`Guardian`)
- `baseunits/WebsiteModules.pas:82` (`TWebsiteModuleAccount`), `:100-180` (`TModuleContainer` fields), `:258-460` (AccountSupport, TotalDirectory, options, cookies merge, limits)
- `docs/LUA-REFERENCE.md:161-275` (MODULE object as modules use it), `:314-347` (Init)
