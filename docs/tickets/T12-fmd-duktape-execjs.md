# T12: `fmd.duktape.ExecJS` via rquickjs
Deps: T03

## Goal
Implement `require('fmd.duktape').ExecJS(code)` with an embedded QuickJS (`rquickjs`), returning what FMD2's Duktape returns, including CommonJS `require` of JS files from the `lua/` tree (e.g. `lua/utils/crypto-js.min.js`).

## Scope (in/out)
In:
- `ExecJS(code) -> string`: evaluate `code` in a fresh JS context; return the completion value converted to string the way Duktape's `duk_safe_to_string` does (`undefined` → `"undefined"`, numbers formatted like JS `String(n)`, objects → `"[object Object]"`, etc.). On a JS error, log it and return nothing (`nil`), matching the Pascal `except` branch.
- CommonJS `require(id)` inside JS, resolving relative to the Lua directory as FMD2's Duktape module loader does (check `Duktape.pas` for search paths and `module.exports` handling).
- Strings: JS uses UTF-16; FMD2 passes UTF-8. Document and test non-ASCII and binary-ish input.
- Execution limits: memory and time limit (interrupt handler) so a runaway script cannot hang a worker; tie into the worker's cancellation token.
- If QuickJS diverges from Duktape on something modules rely on, record it; Duktape via `cc` is the fallback (decision recorded in the plan).

Out: Node-based execution (`lua/utils/nodejs.lua` uses `fmd.subprocess`, T13).

## Seams under test
Lua snippets via the `fmd-lua` runtime:
```lua
local js = require 'fmd.duktape'
assert(js.ExecJS('1+2') == '3')
assert(js.ExecJS('var a=[1,2]; a.join("-")') == '1-2')
assert(js.ExecJS('undefined') == 'undefined')
assert(js.ExecJS('throw new Error("x")') == nil)
assert(js.ExecJS('var C=require("utils/crypto-js.min.js"); C.MD5("a").toString()') == '0cc175b9c0f1b6a831c399e269772661')
assert(js.ExecJS('while(true){}') == nil)           -- interrupted by the limit
```
Also: the upstream modules that call `fmd.duktape` (≈9) evaluate their JS snippets the same as Duktape on recorded inputs (add as fixtures where practical).

## Acceptance criteria
- [ ] Return-value stringification matches Duktape for primitives, arrays, objects, `null`, `undefined`.
- [ ] `require` resolves JS files under the Lua directory like FMD2.
- [ ] Runaway scripts are interrupted; errors never crash the worker.
- [ ] Doc comments cite `LuaDuktape.pas` / `Duktape.pas` lines.

## FMD2 references
- `baseunits/lua/LuaDuktape.pas:14-37` (`ExecJS` wrapper, error handling, lib registration)
- `baseunits/Duktape.pas` (`ExecJS`: context creation, module loader / `require`, result conversion)
- `baseunits/Duktape.Api.pas` (Duktape API bindings used)
- `lua/utils/crypto-js.min.js`, `lua/utils/cryptojs-aes-format.js` (JS files modules `require`)
- `docs/LUA-REFERENCE.md:1140-1158` (`fmd.duktape`)
