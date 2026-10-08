# Runtime: Lua version, state lifecycle, `require`, calling convention

References point at upstream FMD2 `ad3a5b63`.

## 1. Lua version and base environment

* **Lua 5.4.** Release build modes define `-dlua54` (`mangadownloader/md.lpi:293`). `-dluajit` has no build modes attached, so it is effectively off (`md.lpi:292`). `dist/x86_64-win64/lua54.dll` ships alongside it. `docs/LUA-REFERENCE.md` says "Lua Version: 5.4.8". The bindings `#ifdef` between `lua54`, `lua53` and `luajit`. Only 5.4 matters for FMD2r.
* **Case-sensitive names.** Every binding has an optional `{$ifdef luaclass_caseinsensitive}` path, but nothing defines that symbol (grep of `*.pas`, `*.lpi`, `*.inc`). Host names are therefore **case-sensitive** and spelled exactly as listed in [objects.md](objects.md).
* **New-state recipe.** `LuaNewBaseState` (`baseunits/lua/LuaBase.pas:119-130`) builds every state with these steps:
  1. `luaL_newstate()`, then `luaL_openlibs()`. This loads the **full stdlib**: `io`, `os`, `debug`, `package`, `utf8`, `coroutine`, `string`, `table`, `math`.
  2. `LuaBaseRegisterAll` (`LuaBase.pas:84-92`). This defines the globals `print`, `sleep` and `CreateTXQuery`, plus `Trim`, `MaybeFillHost`, `MangaInfoStatusIfPos`, `GetBetween`, `SeparateLeft` and `SeparateRight`. All of them are listed in [globals-and-libraries.md](globals-and-libraries.md). `luaClassRegisterAll` is commented out ("empty right now").
  3. `LuaPackage.RegisterLoader`, which installs the host searcher ([§4](#4-require-packagesearchers-and-caches)).
* **Stdlib use by modules:** `os` is read 49 times in 28 files, `io` 13 times in 5 files, `debug` once (MangaPlus, see [§4.3](#43-chunk-names)), and `require` 637 times in 493 files. `load`, `loadfile`, `dofile`, `collectgarbage` and `package` are never used as globals.
* **No sandboxing.** Map #1 keeps it that way.

## 2. Lua state lifecycle

There are four kinds of state.

### 2.1 Init states (startup, one per module file)

`ScanLuaWebsiteModulesFile` → `TLuaWebsiteModulesLoader.ScanAndLoadFiles` (`LuaWebsiteModules.pas:636-656`) runs at startup:

1. `LuaWebsiteBypass.doInitialization` ([§2.4](#24-the-anti-bot-check-state-one-per-process)).
2. Sets `LuaPackage.LuaLibDir := LUA_REPO_FOLDER`, where `LUA_REPO_FOLDER = <FMD dir>/lua/` (`FMDOptions.pas:297-300`).
3. `FindAllFiles(LUA_WEBSITEMODULE_FOLDER, '*.lua;*.luac', non-recursive)`. Precompiled `.luac` files are accepted too.
4. Starts `min(CPU count, file count)` loader threads. Each thread pulls file names from a shared list (`GetFileName`).
5. Calls `DoInit(file)` for each file (`LuaWebsiteModules.pas:473-500`):
   1. `L := LuaNewBaseState`.
   2. `luaL_loadfile(L, file)` and `lua_pcall(L, 0, 0, 0)` run the **file's top level**. `NewWebsiteModule` does **not exist yet**, so calling it at top level fails.
   3. `lua_getglobal('Init')`. If it is not a function, the error is "no function name "Init()"".
   4. **Only now** is the global `NewWebsiteModule` registered.
   5. `lua_pcall(L, 0, 0, 0)` calls `Init()`. Its return values are discarded: `return m` is harmless and ignored.
   6. `lua_close(L)`. Any Lua-side state created during Init (locals, globals, tables) is **lost**. Only the Pascal-side module objects survive.
6. `NewWebsiteModule()` (`_newwebsitemodule`, `LuaWebsiteModules.pas:467-471`) creates a `TLuaWebsiteModule`. It appends the module to the thread-local `TempModules` list and pushes it as a userdata with the module metatable (`luaWebsiteModuleAddMetaTable`, [objects.md §1](objects.md#1-module--tluawebsitemodule)). An Init may call it several times: 119 `AddWebsiteModule(...)` helper calls create extra modules, and `MangaPark.lua` alone makes 15.
7. After Init (`LoadLuaWebsiteModule`, `LuaWebsiteModules.pas:514-590`):
   * Modules whose `ID` or `Name` is `''` are freed and dropped.
   * If any modules remain, a `TLuaWebsiteModulesContainer{FileName}` is created. Each module is added to the global `Modules` list, `RootURL` is **lower-cased**, and each `On*` string that is non-empty wires the matching Pascal dispatcher (`DoGetInfo`, …).
8. After all threads finish: `LuaPackage.ClearCache` ([§4](#4-require-packagesearchers-and-caches)) and `Modules.Sort`, which sorts by `ID` with `AnsiCompareStr`. `LocateModule` binary-searches this list.
9. Errors in `DoInit` are logged as `LUA>DoInit("file")>`. A file that fails partway still contributes the modules created before the error (they are in `TempModules`).

Usage: `function Init` is defined in 625 files and `NewWebsiteModule` is called in 623.

### 2.2 Worker states (one per OS thread, rebuilt on module switch)

* `GetLuaWebsiteModuleHandler(AModule)` (`LuaWebsiteModuleHandler.pas:59-64`) keeps a **threadvar** `_LuaHandler: TLuaWebsiteModuleHandler`. One handler, and therefore one `lua_State`, exists per thread, created lazily.
* `LoadModule(AModule)` (`:33-54`):
  * If the handler already serves `AModule`, the **same state is reused**: globals, `require`d tables and upvalues persist across hook calls.
  * Otherwise, if it served a different module, `NewHandle` closes the state, runs `__gc`, and creates a fresh one with `LuaNewBaseState`. Then:
    * `LuaDoMe` runs the module file's **top level again** in the new state, from the cached bytecode ([§4.2](#42-module-bytecode-cache)).
    * `LuaPushMe` sets global **`MODULE`** to the module userdata. A *new* metatable is built here, so `MODULE.Account` exists only if `AccountSupport` was enabled.
  * Two modules defined in the **same file** still count as different modules and get separate states.
* Thread exit: `TM.EndThread := @LuaEndThread` frees the handler and closes the state on the thread that owns it (`:88-102`). The comment says `lua_close` must not run `__gc` from another thread.
* GC: after more than 15 `CallFunction`s the handler runs two full `lua_gc(LUA_GCCOLLECT)` cycles (`LuaHandler.pas:374-384`).
* **The stack is never cleared** between hook calls. `TLuaHandler.ClearStack` exists, but the `Do*` dispatchers don't call it, so return values and error messages pile up. This is harmless as long as the host only reads `-1`.
* Threads that host worker states:
  * update-list worker threads (`TUpdateListThread`);
  * the manga-info thread (`uGetMangaInfosThread`);
  * favorites and silent threads (`uFavoritesManager`, `uSilentThread`);
  * download task threads (`TTaskThread`, for `TaskStart` and `GetPageNumber`);
  * per-page download threads (`TDownloadThread`, for `GetImageURL` and the image hooks);
  * the account-check thread (`OnLogin`);
  * the Check Modules threads;
  * any thread running `OnAccountState` (the GUI thread, see `frmAccountManager.pas:299`).

### 2.3 Objects loaded into a worker state

`TLuaHandler.LoadObject(Name, Obj, AddMetaTable)` (`LuaHandler.pas:356-372`) sets global `Name` to a fresh userdata with a fresh metatable. The result is cached by name in a case-sensitive sorted list: if the **same Pascal object** is already loaded under that name, nothing is pushed. A different object under the same name replaces it.

Consequences:

* Because metatables are cached, **field-bound properties capture pointers when the metatable is built.** For example, `TASK.CurrentMaxFileNameLength` binds to `@TaskThread.FCurrentMaxFileNameLength` at load time.
* If the object passed is an `HTTP` object (`THTTPSendThread`), `LoadObject` also stores the handler in `HTTP.LuaHandler`. The anti-bot bypass uses this to run `websitebypass.lua` **inside the module's worker state** ([hooks.md §5](hooks.md#5-anti-bot-hooks-websitebypass)).
* Scalar globals (`URL`, `PAGENUMBER`, `WORKID`, …) are simply re-set before each call with `luaPushStringGlobal`/`luaPushIntegerGlobal`. They are **not cleared afterwards**, so a global from a previous hook stays visible to the next one on that thread.

### 2.4 The anti-bot check state (one per process)

`LuaWebsiteBypass.doInitialization` (`LuaWebsiteBypass.pas:45-75`) does the following:

* If `lua/websitebypass/checkantibot.lua` exists, it creates **one global state** (`LuaNewBaseState`), runs the file and keeps a registry ref to the global function `____CheckAntiBot`. Every HTTP request from every thread calls it under a single critical section, with a GC every 32 calls (`:89-116`).
* It dumps `websitebypass.lua` to bytecode. That chunk runs lazily in each worker state ([hooks.md §5](hooks.md#5-anti-bot-hooks-websitebypass)).

## 3. `Init` and `NewWebsiteModule` contract

* `Init` must be a global function. It is called with no arguments, and returns are ignored.
* `NewWebsiteModule()` takes no arguments and returns a module userdata. The module is created with these defaults (`WebsiteModules.pas:313-330`):

  | Field | Default |
  |---|---|
  | `AccountSupport` | `false` |
  | `SortedList` | `false` |
  | `InformationAvailable` | `true` |
  | `FavoriteAvailable` | `true` |
  | `DynamicPageLink` | `false` |
  | `TotalDirectory` | `1` |
  | `CurrentDirectoryIndex` | `0` |
  | `MaxTaskLimit`, `MaxThreadPerTaskLimit`, `MaxConnectionLimit` | `0` |
  | `Tag` | `0` |
  | `Storage` | empty |

* In practice, Init sets these properties ([usage-counts.md](usage-counts.md) has counts):
  * Always: `ID`, `Name`, `RootURL`, `Category`, `OnGetInfo`, `OnGetPageNumber`, and usually `OnGetNameAndLink`.
  * Optionally: other `On*` hooks, `TotalDirectory`, `SortedList`, `AccountSupport`, `MaxTaskLimit`, `MaxConnectionLimit`, `LastUpdated`, `DynamicPageLink`.
  * It also calls `m.AddOption*(...)`. The option captions are often localized with `require 'fmd.env'.SelectedLanguage` (68 files).
* Within Init, `m.Account` is **never** reachable, even after `m.AccountSupport = true`. The metatable was built before the Account object existed, so the read returns the metatable ([§5.3](#53-__index-read-algorithm-94-129)). No module reads `Account` in Init.
* A hook name refers to a **global function looked up at call time** in the worker state. Assigning a function value instead of a string goes through the string setter, which stores `''` (`lua_tolstring` of a function is `NULL`), so the hook would silently not be registered.

## 4. `require`, `package.searchers` and caches

### 4.1 Searcher installation (`LuaPackage.pas:453-506`)

`RegisterLoader` shifts the existing `package.searchers[1..n]` up by one and inserts `_findpackage` at **index 1**, ahead of the preload searcher. For a module name `p`, `_findpackage` does the following:

1. If `p` starts with `fmd.`, it looks `p` up in `HostPackage`, a sorted `TStringList` that is **case-insensitive**. If found, it returns the library's `luaopen_*` C function as the loader. Lua then calls it with `(name, nil)`, and it returns the library table, which `require` caches in `package.loaded[p]`.

   Registered names: `fmd.env`, `fmd.crypto`, `fmd.duktape`, `fmd.pcre2`, `fmd.strings`, `fmd.logger`, `fmd.fileutil`, `fmd.gzip`, `fmd.imagepuzzle`, `fmd.mangafoxwatermark`, `fmd.subprocess` (each unit's `initialization` section calls `AddLib`).
2. Otherwise it looks `p` up in `Package`, a `TFileCache` (`FileCache.pas`, case-insensitive). On a miss it runs `LoadLuaFile(p)`:
   * The path is `LuaLibDir + p.Replace('.', PathDelim) + '.lua'`. For example, `templates.Madara` becomes `<FMD>/lua/templates/Madara.lua` and `websitebypass.ddos-guard` becomes `lua/websitebypass/ddos-guard.lua`.
   * If the file exists, it is compiled with `luaL_loadfile` in a scratch state and `lua_dump`ed **unstripped** to a memory stream, then cached as `TCachedPackage{FileName, Stream}`.
   * If found, the loader is `luaL_loadbuffer(stream, chunkname=FileName)` (or `luaL_loadfile` with `--lua-dofile`), which returns a Lua function. On a load error it logs `require 'p' LUA_ERR…` and returns 0 values.
3. When nothing is found it returns **0 values**, which Lua treats as `nil`. The standard searchers then run: preload, `package.path` Lua searcher, `package.cpath` C searcher, all-in-one. This is how `require 'pb'` loads the native lua-protobuf module `pb.dll` (`modules/MangaPlus.lua:54`, `utils/protoc.lua:993`) and how `require("utf8")` resolves (`modules/RizzComics.lua:29`).

### 4.2 Module bytecode cache

* **Module files.** `TLuaWebsiteModulesContainer.ByteCode` (`LuaWebsiteModules.pas:713-733`) lazily `lua_dump`s the module file the first time any worker state loads it. The dump is guarded by a critical section and kept for the process lifetime. With `--lua-dofile` (`md.lpr:85`, `AlwaysLoadLuaFromFile`) caching is off and every load reads the file.
* **Packages** (templates, utils, websitebypass libraries) are cached process-wide too, but **`ClearCache` runs after the startup scan**. Bytecode compiled during Init is therefore discarded, and the first runtime `require` recompiles from disk. Edits to `lua/` after startup are not seen until restart, except under `--lua-dofile`.
* `TLuaHandler.LoadChunk`/`LoadChunkExecute` keep a per-state list of chunk names already executed. This is how `websitebypass.lua` runs once per state.
* FMD2r note: caching is an optimisation. The observable contract is only that each worker state runs the module file's top level once, and that `require` caches per state via `package.loaded`.

### 4.3 Chunk names

* `luaL_loadfile` names chunks `@<path>`. The path is absolute in FMD2 because `FMD_DIRECTORY` is absolute and the module list comes from `FindAllFiles(<abs>/lua/modules/)`.
* The bytecode caches dump **unstripped** (`lua_dump(..., strip=0)`, `LuaBase.pas:200`; the comment says "disabled strip to print line info"). In Lua 5.4 an unstripped dump embeds the original `@<path>` source, and `luaL_loadbuffer` keeps it. So `debug.getinfo(1,'S').source` returns `@<absolute path>` for cached and uncached loads alike.
* `modules/MangaPlus.lua:106-110` relies on this to find `MangaPlus.proto` next to itself: it takes `source`, swaps the script name for the proto name and strips `@`. FMD2r must load module chunks with chunkname `@<real path>` and keep that name through any caching.

## 5. The userdata calling convention (`LuaClass.pas`)

### 5.1 Object representation

`luaClassPushUserData` / `luaClassNewUserData` (`LuaClass.pas:284-330`):

* The **userdata** is a block holding one pointer to the Pascal object (`luaPushUserData`, `LuaUtils.pas:296-299`).
* Each push creates a **brand-new metatable**, with no sharing between objects of the same class. It contains:
  * `self`: a closure that returns the object pointer as light userdata;
  * `__index` and `__newindex`: the generic dispatchers below;
  * `__gc`;
  * `__autodestroy`: a boolean, true for objects Lua owns.
* The class-specific `AddMetaTable` function then adds the members:

  | Kind | Stored as `metatable[name]` |
  |---|---|
  | Method | A C closure with upvalue 1 = the userdata |
  | Getter/setter property | A table `{__get = closure(ud), __set = closure(ud)}`, either field optional |
  | Field-bound property | A table `{__get, __set}` whose closures carry upvalue 1 = a light userdata pointing straight at the Pascal field (`luaClassAddVariable`, `:500-518`) |
  | Child object | Another full userdata (`luaClassAddObject`), with its own fresh metatable, created **once at push time**. `HTTP.Headers` is the same userdata on every read. |
  | Array property | A proxy table whose metatable maps `__index` / `__newindex` to `getter(ud, key)` / `setter(ud, key, value)` (`luaClassAddArrayProperty`, `:399-433`) |
  | Default array property | `__defaultget` / `__defaultset` closures in the metatable (`:448-455`) |
* `__gc` (`:203-230`) acts only when `__autodestroy` is true. It then calls the metatable's `destroy` C function if there is one, otherwise it frees the Pascal object. Objects created from Lua are auto-destroyed: TXQuery, IXQValue, `fmd.strings` lists, ImagePuzzle and TProcess. Host-injected objects (`HTTP`, `TASK`, …) are not.

### 5.2 Method calls: upvalue-bound self, dot syntax

`luaClassGetClosure` (`:297-309`) uses **upvalue 1** as the object when it is a userdata, which every method closure has. Only when there is no such upvalue does it take argument 1 and remove it. That fallback is used by array-property accessors and `__defaultget`/`__defaultset`, which are called with `(ud, key[, value])`.

So `HTTP.GET(url)` receives `url` at index 1. `HTTP:GET(url)` would receive `(HTTP, url)`, read the userdata as the URL, `lua_tolstring(userdata)` would give `NULL`, and the request would use `''`.

**Colon calls on host objects do not work. The `lua/` tree has 0 colon calls on host members**, against thousands of dot calls ([usage-counts.md](usage-counts.md)). FMD2r should bind methods so dot calls work. Supporting colon calls as well is optional, but making them behave differently from upstream won't affect any module.

### 5.3 `__index` read algorithm (`:94-129`)

Given `obj[key]`:

1. `v = rawget(mt, tostring(key))`. A numeric key `0` is converted to the string `"0"` for this lookup only.
2. If `v` is a **table**, get `v.__get`.
   * If that is a C function, call it with 0 args and return its result.
   * Otherwise return `v.__get`. For a read-only-without-getter property that is `nil`. For an **array property** it is the proxy table itself, because the proxy's `__index('__get')` returns the proxy (`__indexarray`, `:171-185`). So `obj.Values` yields the proxy and `obj.Values['k']` calls the getter.
3. If `v` is **nil**, use `mt.__defaultget`.
   * If present, call `__defaultget(key)` with the **original** key (number or string) and return the result. This applies to TStrings (`NAMES[0]`) and `Storage['k']`.
   * If absent, **the function returns the metatable itself**. The stack still holds `mt`, so `return 1` yields `mt`. Reading an unknown member of an object without a default getter returns a non-nil table. That table is truthy and indexing it returns raw metatable entries. Examples: `HTTP.Foo`, or `m.Account` while Account is absent.
4. Otherwise (a function, userdata or boolean) return `v` as-is. Methods and child objects come back this way. So do the internal names `self`, `__index`, `__newindex`, `__gc`, `__autodestroy`, `__defaultget`, `__defaultset`, which all **shadow** same-named keys: `NAMES.self` is the method, not `NAMES[...]`.

### 5.4 `__newindex` write algorithm (`:131-169`)

Given `obj[key] = value`:

1. `v = rawget(mt, tostring(key))`.
2. If `v` is a table and `v.__set` is a C function, call `__set(value)`. If `__set` is missing (a read-only property), **nothing happens**.
   * For array-property proxies, `getfield(proxy, '__set')` calls the getter with key `"__set"`. The result isn't a C function, so nothing happens.
3. If `v` is nil and `mt.__defaultset` exists, call `__defaultset(key, value)`. Otherwise **nothing happens**. This is the **silently ignored unknown setter**: `HTTP.FollowRedirection = false` in `websitebypass/cloudflare.lua:134`, and `MANGAINFO.Artist = …` in `modules/OrckuMangas.lua:91`.
4. If `v` is anything else (a method or child object), **nothing happens**. Assigning over a method or child object is ignored.

No host object ever raises a Lua error for an unknown key.

### 5.5 Type coercion at the boundary

| Pascal read | Lua → host conversion | Notes |
|---|---|---|
| `luaToString(L, i)` (`LuaUtils.pas:313-320`) | `lua_tolstring`, then `PAnsiChar` → `String` | **Truncates at the first NUL byte.** Numbers become their string form. `nil`, booleans, tables and userdata become `''`. |
| Binary-safe string reads | `lua_tolstring` with length | `fmd.crypto` (`GetLuaString`, `luaL_checklstring`, which **raises a Lua error** for non-strings), `fmd.gzip.Inflate`, `Document.WriteString`, `fmd.pcre2` subject and pattern |
| `lua_tointeger` | Lua 5.4 rules | Integer-valued floats and numeric strings convert. **Non-integral floats and non-numeric values give 0.** For example, `TASK.PageNumber = n/2` with odd `n` stores 0. |
| `lua_toboolean` | Lua truthiness | The string `'false'` is true. |
| `luaToUserData(L, i)` | `*(void**)lua_touserdata` | Used where the host expects a stream, list, IXQValue or other host object passed as an argument. No type check: a wrong type or `nil` dereferences garbage or NULL (an access violation, which becomes an exception, see [§6](#6-error-handling)). |

| Pascal write | Host → Lua conversion | Notes |
|---|---|---|
| `lua_pushstring(L, AnsiString)` (`lua54.pas:647-650`) | `lua_pushlstring(ptr, Length)` | Binary-safe on the way **into** Lua. |
| `lua_pushinteger` / `lua_pushboolean` | Native | Integer properties are 32-bit Pascal `Integer`s. |

Pascal strings are byte strings and FMD2 treats them as UTF-8. No transcoding happens at the boundary.

## 6. Error handling

* **Hook calls.** `LuaCallFunction` (`LuaBase.pas:132-145`) uses `lua_pcall(L, 0, LUA_MULTRET, 0)`. A Lua error, or a missing function ("No function name "X""), becomes a Pascal exception. The `Do*` dispatcher logs it as `LUA>DoGetInfo("file.lua")>` and returns its **default**: `INFORMATION_NOT_FOUND` (2) for byte hooks, `False` for boolean hooks, `''` for `OnSaveImage`. There is no message handler, so no traceback is attached.
* **Host-side failures inside a host function** (TStrings index out of range, access violations from bad userdata arguments, `IXQValue.GetAttribute` on a non-node, `Delimiter = ''`) raise **Pascal exceptions inside the C callback**. They unwind through the Lua C frames to the dispatcher's `try/except`, so the whole hook fails with the default above. Lua `pcall` inside the module cannot intercept them.
  * FMD2r can map these to Lua errors. That is a strict improvement: modules that `pcall` would recover, and every other module sees the same "hook failed" outcome.
* **Errors that are swallowed and return a neutral value instead:**
  * XPath evaluation errors give an empty sequence (`XQueryEngineHTML.pas` `Eval`, `try … except end`).
  * `fmd.duktape.ExecJS` returns **no value** and logs `Duktape.ExecJS() …`.
  * `fmd.gzip.Inflate` returns no value and logs.
  * Network exceptions inside `HTTPRequest` give `false`.
  * `fmd.pcre2` compile errors are logged and return `false` or no values.
* **Undefined globals are `nil`.** These names are read as globals but are never assigned as globals anywhere in `lua/` (luac listing, [usage-counts.md](usage-counts.md#2-global-reads)): `net_error`, `WorkId`, `Module`, `tointeger`, `langs`, `splitstr`, `bit32_band`, `lshift`, `StringUnscramble`, `NextJs`. Most of them sit on rarely taken paths, and they evaluate to `nil`. FMD2r must **not** install a strict-globals guard.
