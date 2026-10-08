# T03: LuaClass binding helper
Deps: T01

## Goal
Build, in `fmd-lua`, the one generic mechanism through which every Host API object (HTTP, MODULE, MANGAINFO, TASK, TStrings, …) is exposed to Lua, reproducing the object semantics of FMD2's `LuaClass.pas` exactly. Every later Host API ticket builds on this, so its behaviour must match FMD2 precisely.

## Scope (in/out)
In:
- `fmd-lua` crate setup: `mlua` with vendored Lua 5.4.
- A public runtime entry point that later tickets extend, e.g. `fmd_lua::Runtime::new() -> Result<Runtime>` and `Runtime::exec(&self, chunk: &str) -> Result<()>` / `Runtime::eval<T>(&self, expr: &str) -> Result<T>`.
- A `LuaClass` builder over mlua userdata + metatables that registers, per object type:
  - **methods** as closures bound to the object, so `obj.Method(a)` (dot call) works; a redundant leading self argument is stripped so `obj:Method(a)` also works;
  - **properties** (getter/setter pairs) through `__index` / `__newindex`, including typed helpers for string, integer and boolean fields;
  - **array properties** (`obj.Prop[i]`) and a **default array property** that receives unknown integer/string keys (`obj[i]`);
  - **sub-objects** (an object exposed as a property of another, e.g. `MODULE.Storage`);
  - `obj.self()` on every object (returns the object itself);
  - **unknown keys**: reading an unknown key falls through to the default array property if one exists, otherwise yields `nil`; writing an unknown key is **silently ignored** (no error).
- Keys are case-sensitive (no case folding).
- Strings are binary-safe (Lua strings ↔ `Vec<u8>`/`bytes`; no truncation at NUL).
- A tiny test-only object type registered through the helper to exercise every feature.

Out: concrete Host API objects (T04 onward).

## Seams under test
Lua snippets executed through the public `fmd-lua` runtime API against a test object registered with the `LuaClass` builder, e.g.:
```lua
assert(obj.Add(1, 2) == 3)           -- dot call
assert(obj:Add(1, 2) == 3)           -- colon call, self stripped
obj.Name = 'x'; assert(obj.Name == 'x')
obj.DoesNotExist = 5                  -- silently ignored
assert(obj.DoesNotExist == nil)
assert(obj.name == nil)               -- case-sensitive
assert(obj[0] == 'first')             -- default array property
assert(obj.self() == obj)
assert(#obj.Bytes('a\0b') == 3)       -- binary safe
```

## Acceptance criteria
- [ ] Dot and colon calls both work for every method, including methods with zero arguments.
- [ ] Properties read/write through `__index`/`__newindex`; typed string/int/bool helpers exist.
- [ ] Unknown-key reads return `nil` or route to the default array property; unknown-key writes are ignored without error.
- [ ] `.self()` exists on every object built with the helper.
- [ ] Keys are case-sensitive.
- [ ] NUL bytes survive a round trip through a method argument and return value.
- [ ] Each behaviour has a doc comment citing the `LuaClass.pas` line it reproduces.

## FMD2 references
- `baseunits/lua/LuaClass.pas:94` (`__index`: method/property lookup, fallthrough to default array property)
- `baseunits/lua/LuaClass.pas:131` (`__newindex`: setters, unknown keys ignored)
- `baseunits/lua/LuaClass.pas:171`, `:187` (`__indexarray` / `__newindexarray`)
- `baseunits/lua/LuaClass.pas:232` (`__self`)
- `baseunits/lua/LuaClass.pas:297-311` (`luaClassGetClosure`/`luaClassGetObject`: closures bound to the object)
- `baseunits/lua/LuaClass.pas:341-462` (`luaClassAddFunction`, `luaClassAddProperty`, `luaClassAddArrayProperty`, `luaClassAddDefaultArrayProperty`)
- `baseunits/lua/LuaClass.pas:464-560` (string/int/bool property helpers, `luaClassAddObject`, `luaClassAddUserData`)
- `baseunits/lua/LuaUtils.pas:206` (`luaToString`, string conversion)
- `baseunits/lua/LuaBase.pas:119` (`LuaNewBaseState`)
