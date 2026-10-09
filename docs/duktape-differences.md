# `fmd.duktape.ExecJS`: QuickJS against FMD2's Duktape

FMD2 runs `ExecJS` on Duktape 2.3.0 (`baseunits/Duktape.pas:77-104`, `DUK_VERSION = 20300` in
`baseunits/Duktape.Api.pas:431`). FMD2r runs it on QuickJS (`rquickjs`). T50 compared the two:
`crates/fmd-duktape-ref` builds Duktape 2.3.0 from its release sources as a test-only reference,
and `crates/fmd-lua/tests/duktape_reference.rs` runs the same scripts through `fmd.duktape` and
through that build and compares the bytes that come back.

## What the comparison covers

- Strings: non-BMP characters, `length`, `charCodeAt`, slicing, regular expressions on astral
  characters, and the bytes returned to Lua.
- JSON round trips, including every UTF-16 code unit through `JSON.stringify`.
- The `Duktape` built-in's codecs, `TextEncoder`/`TextDecoder`, and the `btoa`/`atob` that
  `websitebypass/cloudflare.lua` builds on them.
- The global object and every built-in's own members.
- `require` (Duktape's module loader with FMD2's `modSearch`).
- `Function.prototype.toString`.
- Date formatting and parsing.
- Real pages run through the modules' own code:
  - ac.qq.com and ReadComicOnline (`fixtures/js`);
  - FanFox (the smoke recording);
  - three captured Cloudflare IUAM challenges.

  Every `ExecJS` those modules make is compared.

The reference handles dates like FMD2's Windows build, not like a Linux build (see
`crates/fmd-duktape-ref/src/duktape_fmd2.c`).

## Reproduced on QuickJS

The heap setup (`crates/fmd-lua/src/duktape/`) makes QuickJS match Duktape in these:

| Behaviour | Duktape 2.3 | Where |
|---|---|---|
| A non-BMP character in the result | CESU-8: each surrogate as 3 bytes, not 4-byte UTF-8 | `duktape.rs` (`cesu8`) |
| `JSON.stringify` | lone surrogates written as they are; U+2028/U+2029 escaped | `builtins.js` |
| `Duktape` | `version`, `env` (FMD2's DLL's), `enc`/`dec` (`hex`, `base64`), `gc`, `compact`, `modLoaded`, `modSearch` | `builtins.js`, `prelude.js` |
| `TextEncoder`, `TextDecoder` | UTF-8 only, label ignored; `fatal`, `ignoreBOM`, `stream` | `builtins.js` |
| `Uint8Array.allocPlain`, `plainOf` | plain buffers (a `Uint8Array` here) | `builtins.js` |
| Built-in members | exactly Duktape's: no `Symbol`, `Map`, `Promise`, `globalThis`, `atob`, `Array.prototype.find`, `String.prototype.big`, …; typed arrays print as `[object Uint8Array]` | `surface.js` |
| `require` | non-enumerable global; no `prototype`; string ids only; reads `Duktape.modLoaded`/`modSearch` on each call; wrapper named after the id's last term | `prelude.js` |
| `Function.prototype.toString` | `function NAME() { [ecmascript code] }` (`[native code]`, `[bound code]`) | `builtins.js` |
| `Date` to string | `2020-06-07 08:09:10.011+02:00` forms; `toLocale*String` the same (no strftime on Windows) | `builtins.js` |
| `Date.parse`, `new Date(string)` | Duktape's ISO 8601 subset only, a time without offset is UTC, anything else `NaN` | `builtins.js` |
| `Date.UTC(year)` | `NaN` (ES5) | `builtins.js` |

## Differences that remain

`known_differences_from_duktape` in `duktape_reference.rs` pins each one that is deterministic.
For each difference, "why no module observes it" refers to:

- the JS of the eight upstream call sites: `templates/Madara.lua`, `modules/FanFox.lua`,
  `MangaGo.lua`, `ZeroScans.lua`, `DigitalTeam.lua`, `acqqcom.lua`, `ReadComicOnline.lua` and
  `websitebypass/cloudflare.lua`;
- the JS files they `require` (`utils/crypto-js.min.js`, `utils/cryptojs-aes-format.js`);
- the page scripts recorded for them.

None of these use the constructs below.

| Difference | Why no module observes it |
|---|---|
| ES2015+ syntax (`let`, arrows, classes, template literals, destructuring, `for…of`, regex flags `y`/`u`/`s`, named groups) parses here; Duktape throws a SyntaxError | A script with it fails under FMD2, so no working module sends one. Here it runs instead of failing, which no module can depend on. |
| Error messages and `stack` text (`undefined_var is not defined` vs `identifier 'undefined_var' undefined`) | `ExecJS` returns no value on any error, whatever the message. No module script reads `e.message` or `e.stack`. |
| `Array.prototype.sort` order of equal elements | Duktape sorts with a random pivot, so the order varies between runs in FMD2 itself. Any order QuickJS gives is one Duktape can give. `ReadComicOnline.lua` sorts candidate strings by length and takes the first; ties are already arbitrary in FMD2. |
| Node.js `Buffer` | Browser page scripts don't use it. No module script references it. |
| `Duktape.Pointer`, `Thread`, `act`, `fin`, `info`; `Duktape.enc`/`dec` formats `jx`/`jc`; `Error.prototype.fileName`/`lineNumber`; functions' own `fileName` | Duktape internals: only `cloudflare.lua`'s `Duktape.enc('base64')`/`dec('base64')` is used, and that is reproduced. |
| Decimal literals beyond 2^53 (`9007199254740993` → `…992` here, `…994` on Duktape) | Duktape's conversion is off by one unit. Numbers past 2^53 lose precision in both engines anyway, and no module script has such literals. |
| `String.fromCharCode(c)` with `c` above U+FFFF keeps the low 16 bits here; Duktape keeps the code point (`DUK_USE_NONSTD_STRING_FROMCHARCODE_32BIT`) | Every call in the module scripts masks its argument to 8 or 16 bits (`255 & …`, `& 65535`). |
| `/[/]/.source` is `[/]` here, `[\/]` on Duktape | No module script reads `source`. |
| An anonymous function expression is named after its variable here (`var f = function () {}` → `f.name === 'f'`); Duktape leaves it unnamed, so `toString` differs too | No module script reads function names or prints functions. |
| Own properties of instances: typed arrays' `length` and strict `arguments.caller` are own on Duktape, inherited or absent here; symbol-keyed built-in members | Seen only through `Object.getOwnPropertyNames`/`getOwnPropertySymbols` on such objects, which no module script calls. |
| Getters receive the property name as an argument on Duktape (`DUK_USE_NONSTD_GETTER_KEY_ARGUMENT`) | No module script defines a getter that reads its arguments. |
| Local time far from the present: Duktape maps years outside about 1970–2037 to an equivalent year for the time zone offset; QuickJS uses the time zone database (e.g. local mean time before 1900). Duktape on Windows also reads the offset from the Windows API | Only `Date` objects with local-time getters or `toString` show it. No module script uses `Date`. |
| `Duktape.env` describes FMD2's DLL here (`ll u nl p2 a8 x64 windows mingw`) | Not a difference from FMD2. The Linux reference build reports itself instead. |

Not compared: `print` output (it goes to the log only) and resource limits (Duktape has none;
FMD2r bounds time and memory, see `JsLimits`).
