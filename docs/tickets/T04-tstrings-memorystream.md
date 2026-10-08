# T04: TStrings (`fmd.strings`) and MemoryStream objects
Deps: T03

## Goal
Expose FMD2's TStringList and TMemoryStream objects to Lua with the exact surface and quirks modules rely on. They are used everywhere: `MANGAINFO.ChapterLinks`, `TASK.PageLinks`, `HTTP.Headers`, `HTTP.Document`, and `require 'fmd.strings'`.

## Scope (in/out)
In:
- A Rust `LuaStrings` type registered via the T03 `LuaClass` helper, with the full surface from `LuaStrings.pas`: `LoadFromFile`, `LoadFromStream`, `SaveToFile`, `SaveToStream`, `Text` (get/set), `CommaText`, `Add`, `AddText`, `Get`, `Set`, `DelimitedText`, `Delimiter`, `NameValueSeparator`, `Values[name]`, `Count`, `Sort`, `Clear`, `Delete`, `IndexOf`, `IndexOfName`, `Reverse`, and the **0-based default array property** (`list[0]` is the first item).
- Name/value semantics matching TStrings (`Values['k']` reads/writes `k=v` lines; setting to `''` writes `k=`, as FPC 3.2.2 does, rather than removing the line as Delphi does).
- `Text` joins with line endings and splits on CR/LF/CRLF like FPC's TStrings.
- `require 'fmd.strings'` returning a lib with `New()` that creates a standalone list.
- A Rust `LuaMemoryStream` with `ToString`, `WriteString`, `LoadFromFile`, `SaveToFile`, `Size` (get/set), `Clear`, binary-safe.
- Public Rust handles so other objects can own and share these (e.g. MANGAINFO owns several lists; HTTP owns a stream).

Out: the package searcher in general (T06; here, register `fmd.strings` through whatever minimal hook T03 offers or a temporary preload); HTTP-specific behaviour (T10).

## Seams under test
Lua snippets through the `fmd-lua` runtime API:
```lua
local s = require('fmd.strings').New()
s.Add('a'); s.Add('b')
assert(s.Count == 2 and s[0] == 'a' and s[1] == 'b')   -- 0-based
assert(s.Get(1) == 'b')
s.Values['k'] = 'v'; assert(s.Values['k'] == 'v'); assert(s.IndexOfName('k') == 2)
s.Reverse(); assert(s[0] == 'k=v')
s.Text = 'x\r\ny\nz'; assert(s.Count == 3)
s.CommaText = 'a,"b c",d'; assert(s[1] == 'b c')
```
And the MemoryStream: `m.WriteString('a\0b'); assert(m.Size == 3)`, with `ToString()` returning `'a\0b'` once the owner (the public Rust handle) moves the position back to 0. Like FMD2's `StreamToString`, `ToString` reads from the position, which writing leaves at the end.

## Acceptance criteria
- [ ] Default index on TStrings is 0-based; out-of-range reads behave as in FMD2 (document what FPC does and match it).
- [ ] Every method listed in `LuaStrings.pas` is present and callable with dot or colon syntax.
- [ ] `CommaText`, `DelimitedText`, `Delimiter`, `NameValueSeparator` follow FPC TStrings quoting rules (cover with cases taken from FPC behaviour).
- [ ] MemoryStream is binary-safe.
- [ ] Doc comments cite `LuaStrings.pas` / `LuaMemoryStream.pas` lines.

## FMD2 references
- `baseunits/lua/LuaStrings.pas:20-255` (each `strings_*` function: create, load/save, Text, CommaText, Add, Get/Set, Delimited*, Values, Count, Sort, Clear, Delete, IndexOf, IndexOfName, Reverse)
- `baseunits/lua/LuaStrings.pas:257-266` (metatable: default array property; `luaopen_strings`)
- `baseunits/lua/LuaMemoryStream.pas:21-90` (MemoryStream methods and metatable)
- `docs/LUA-REFERENCE.md:907-935` (LINKS/NAMES TStringList usage from modules)
- FPC RTL `TStrings` (`classes` unit, `stringl.inc`) for CommaText/DelimitedText/Values semantics
