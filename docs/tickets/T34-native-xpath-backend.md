# T34: Native Rust XPath backend
Deps: T08

## Goal
Implement a pure-Rust `XPathEngine` backend (`native`) so FMD2r can eventually drop the FPC shim: HTML parsed with `html5ever` into a tree, XPath 3.1 evaluated with `xee` (or the best available Rust engine), plus the Xidel/internettools extensions upstream modules use.

## Scope (in/out)
In:
- `fmd-xpath` `native` backend behind a cargo feature, implementing the same trait as T08 so the Lua bindings are unchanged.
- HTML parsing/repair as close as practical to internettools' `pmHTML` with repair on, `trimText=false`, no comments/PIs (document known differences).
- Extensions modules rely on: `json()` / `parse-json`, dot-path navigation into JSON (`$x.a.b`, `?key`, `?*` lookups), `jn:*` functions (`jn:keys`, `jn:members`, …), `css()` selectors, string-join semantics, implicit atomisation as internettools does. Inventory them by scanning the upstream corpus (T02) for XPath strings and list each feature with a test.
- Error behaviour: evaluation errors yield empty values (same as FPC backend).
- Backend selection setting (`xpath.backend = fpc|native`), default stays `fpc` (T35 flips it).

Out: the differential corpus and switch (T35).

## Seams under test
- The `fmd-xpath` trait API, run as a **shared test suite against both backends** (parameterised): every case from T08's suite plus extension cases, e.g. `json('{"a":{"b":[1,2]}}')?a?b?*` → `(1,2)`; `css('div.x > a')`; `$json.a.b` dot-path; `jn:keys(json('{"x":1}'))` → `x`.
- Lua snippets through `fmd-lua` with the native backend selected produce the same results as the FPC backend for the T08 Lua tests.

## Acceptance criteria
- [ ] Shared suite passes on both backends (or native failures are listed as known gaps in the crate README with issue links/ticket ids).
- [ ] No FPC dependency when only `native` is enabled.
- [ ] Extension inventory from the corpus committed as a doc.

## FMD2 references
- `baseunits/XQueryEngineHTML.pas:384-400` (parser config to emulate), `:236-330` (eval/error semantics)
- `baseunits/lua/LuaXQuery.pas`, `baseunits/lua/LuaIXQValue.pas` (behaviour the bindings expect)
- `docs/LUA-REFERENCE.md:1022-1056` (XPath extensions used by modules), `:1408-1425` (JSON API handling)
- internettools source (https://github.com/benibela/internettools, `data/xquery*.pas`, `data/simplehtmltreeparser.pas`) for exact extension semantics
