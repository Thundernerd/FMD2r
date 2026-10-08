# T08: `fmd-xpath` trait, FFI backend, `CreateTXQuery`/`IXQValue` Lua bindings
Deps: T03, T07

## Goal
Give Rust a backend-agnostic XPath API (`trait XPathEngine`) with an FFI backend over `libfmdxpath.so`, and expose it to Lua as FMD2's `CreateTXQuery(...)` / TXQuery object / `IXQValue` with identical semantics.

## Scope (in/out)
In:
- `fmd-xpath`: `trait XPathEngine { type Doc; type Value; fn parse(&self, html: &[u8]) -> Result<Doc>; fn eval(&self, doc, expr, ctx: Option<&Value>, css: bool) -> Value; ... }` (shape it as you see fit; keep it object-safe or provide a boxed wrapper) with the value operations: count, get(1-based), to_string, inner/outer HTML, inner text, get_attribute, get_property.
- `fpc` backend: safe Rust wrapper over the T07 C ABI (`build.rs` builds or locates the `.so`; feature-gated so the workspace still builds without FPC, with tests skipped and a clear message).
- Optional query logging hook (expression + document hash) for the T35 differential corpus.
- Lua bindings in `fmd-lua`:
  - Global `CreateTXQuery([html_string | memory_stream])`.
  - TXQuery methods: `ParseHTML(string|stream)`, `XPath(expr[, context])` → IXQValue, `XPathString(expr[, context])`, `XPathStringAll(expr[, sep][, context])` and the `XPathStringAll(expr, tstrings[, context])` form that fills a TStrings, `XPathHREFAll(expr, links, names)`, `XPathHREFTitleAll(expr, links, names)`, `XPathCount(expr[, context])`.
  - `XPathStringAll` semantics: trims each item, skips empty ones, joins with `', '` by default.
  - IXQValue: `Get(i)` 1-based; `Get()` with no arguments returns an iterator (yields nothing when count is 0); `Count` property; `GetAttribute`, `GetProperty`, `InnerHTML`, `OuterHTML`, `InnerText`, `ToString`.
  - Argument overloads dispatch on Lua type and argument count exactly like the Pascal (`lua_gettop` cases).

Out: native backend (T34); differential corpus (T35).

## Seams under test
- Public `fmd-xpath` API with the `fpc` backend (Rust tests): parse/eval/count/get on small documents.
- Lua snippets through the `fmd-lua` runtime:
```lua
local x = CreateTXQuery('<div><a href="/1">One</a><a href="/2"> </a><a href="/3">Three</a></div>')
assert(x.XPathCount('//a') == 3)
assert(x.XPathString('//a[1]/@href') == '/1')
assert(x.XPathStringAll('//a') == 'One, Three')        -- trimmed, empties skipped
local n = 0; for v in x.XPath('//a').Get() do n = n + 1 end; assert(n == 3)
assert(x.XPath('//a').Get(1).GetAttribute('href') == '/1')
local links, names = require('fmd.strings').New(), require('fmd.strings').New()
x.XPathHREFAll('//a', links, names); assert(links[0] == '/1')
assert(x.XPath('json("{\\"k\\":1}")?k').ToString() == '1')
assert(x.XPathCount('//a[') == 0)                       -- errors yield empty
```

## Acceptance criteria
- [ ] All TXQuery methods and overloads from `LuaXQuery.pas` exist and dispatch identically.
- [ ] IXQValue iterator and 1-based `Get` match `LuaIXQValue.pas`.
- [ ] Invalid expressions return empty results, never a Lua error (match FMD2).
- [ ] Workspace builds and tests pass without FPC installed (backend feature off); CI runs with it on.
- [ ] Doc comments cite the Pascal lines.

## FMD2 references
- `baseunits/lua/LuaXQuery.pas:22-43` (`CreateTXQuery` overloads), `:45-56` (`ParseHTML`), `:58-90` (`XPath`, `XPathString`), `:91-136` (`XPathStringAll` overloads), `:137-164` (`XPathHREFAll`, `XPathHREFTitleAll`), `:165-188` (`XPathCount`), `:178-199` (method table, global registration)
- `baseunits/lua/LuaIXQValue.pas:37-150` (IXQValue methods; `Get` iterator vs 1-based index), `:151-160` (`Count` property)
- `baseunits/XQueryEngineHTML.pas:130-137` (`AddSeparatorString`), `:332-382`, `:462-545` (StringAll/HREF helpers: trimming, skipping empties, default separator)
- `docs/LUA-REFERENCE.md:993-1056`, `:1490-1512` (TXQuery usage and iterating nodes)
