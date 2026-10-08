# XPath engine: upstream usage and options for FMD2r

Research for [#4](https://github.com/Thundernerd/FMD2r/issues/4), part of map [#1](https://github.com/Thundernerd/FMD2r/issues/1).
Upstream baseline: FMD2 `ad3a5b63` (2026-10-04), checked out at `~/Repositories/Forks/FMD2`. Facts gathered 2026-10-08.

## TL;DR

- Upstream modules use a **small, regular slice** of the engine. That slice is XPath 1.0/2.0 paths and predicates, plus about 20 functions, plus Internet Tools' **JSONiq-style JSON navigation** (`json(*).data().title`, `?*`, bare property names on objects).
- Across all 3,607 call sites, **no module uses FLWOR, `let`, variables, map or array constructors, `if`, quantifiers, type operators, `matches`, or any XQuery syntax**.
- No off-the-shelf engine covers the slice:
  - Every Rust and C engine lacks the JSONiq navigation, which appears in 144 of 347 files.
  - SaxonC-HE and xee have no HTML parser.
  - libxml2 and sxd-xpath stop at XPath 1.0.
- Shipping Internet Tools itself through FFI would give the best fidelity, but it is **GPLv3-or-later**. FMD2r's `LICENSE` is GPL-2.0 with no "or later" grant on record, so this is a licence blocker before it is a technical one.
- **Recommendation: (d) a purpose-built Rust subset.**
  - Build it on a lenient HTML parser (html5ever) and `serde_json`. Optionally lift the parser and AST from a Skyscraper fork.
  - Use Xidel / Internet Tools only as a **differential-testing oracle** in CI or dev, never shipped. That avoids the GPLv3 question.
- Under this recommendation the "Pascal exception" in #1 is **not needed**.

## 1. How FMD2 wires the engine

### Lua surface

Source: `baseunits/lua/LuaXQuery.pas` and `baseunits/lua/LuaIXQValue.pas`.

- `CreateTXQuery([html|stream])` returns an object with these methods:
  - `ParseHTML`
  - `XPath(expr[, ctx])` returns an IXQValue
  - `XPathString(expr[, ctx])`
  - `XPathStringAll(expr[, sep|TStrings[, ctx]])`
  - `XPathHREFAll(expr, links, texts[, ctx])`
  - `XPathHREFTitleAll(expr, links, titles[, ctx])`
  - `XPathCount(expr[, ctx])`
- The IXQValue userdata exposes:
  - methods `Get([i])` (an index or an iterator), `GetAttribute`, `GetProperty`, `InnerHTML`, `OuterHTML`, `InnerText` and `ToString`
  - the property `Count`
- `css()` is not a Lua method. Modules call it as an XPath function inside the expression, e.g. `x.XPathString('css("div#info > h1")')`. `TXQueryEngineHTML.CSS*` exists in Pascal but is not exported to Lua.

### Wrapper semantics

Source: `baseunits/XQueryEngineHTML.pas`. A Rust port must reproduce each of these behaviours exactly.

- **Errors:** `Eval` wraps evaluation in `try … except end`. Any parse or evaluation error silently yields the empty sequence, and `XPathString` then yields `''`. Modules rely on this, for example with `tonumber(x.XPathString(...)) or 1`.
- **Joining in `XPathStringAll`:**
  - It skips items whose trimmed string is empty.
  - It trims each item and joins with `', '` by default.
  - When the target is a `TStrings`, it adds the trimmed items without skipping empties.
- **`XPathHREFAll`:** adds `node.getAttribute('href')` and the trimmed string value. `XPathHREFTitleAll` uses `@href` and `@title`.
- **Context:** the context is either the explicitly passed IXQValue (a node *or a JSON object or array*) or the root of the last parsed tree.

### Engine configuration

- `TTreeParser` is set up as follows:
  - `parsingModel := pmHTML`
  - `repairMissingStartTags := True`, `repairMissingEndTags := True`
  - `trimText := False`
  - `readComments := False`, `readProcessingInstructions := False`
  - `autoDetectHTMLEncoding := False`
- `pmHTML` is Internet Tools' own heuristic parser: "accept everything, tries to create the best fitting tree using a heuristic to recover from faulty documents (no exceptions are raised)". It is **not** the WHATWG tree-construction algorithm.
  - Source: [`simplehtmltreeparser.pas`](https://github.com/benibela/internettools/blob/master/data/simplehtmltreeparser.pas), line ~343.
  - It never synthesises `<tbody>`; the string does not appear in the parser.
- `TXQueryEngine.Create` defaults ([`xquery.pas`](https://github.com/benibela/internettools/blob/master/data/xquery.pas), `constructor TXQueryEngine.create`):
  - `AllowPropertyDotNotation := xqpdnAllowUnambiguousDotNotation`. This enables `$obj.prop`.
  - `AllowJSONLiterals := true`, so `true`, `false` and `null` work as literals.
  - `AllowExtendedStrings := true`.
  - `jsonPXPExtensions := true`. This allows "child and descendant axis test matching object properties", so a bare `title` against an object context reads that property.
  - `AllowJSONiqOperations := true` once `xquery_json` is linked, which FMD2 does. This allows implicit casts of objects and arrays to boolean or string.
  - The default JSON parser runs with `jpoLiberal, jpoAllowTrailingComma, jpoAllowMultipleTopLevelItems, jpoJSONiq`.
  - The default function namespace merges `fn:` with Internet Tools' `pxp:` extensions, which is why `json(...)` and `css(...)` resolve unprefixed.
  - `strictTypeChecking` is off by default: "things like `"2" + 3` … evaluated to 5".
- **`evaluateXPath` parses as XPath 4.0.** At current master it calls `evaluate(expression, xqpmXPath4_0, …)`, so FMD2 effectively runs an "XPath 4.0 + JSONiq + PXP extensions" dialect, not XPath 3.1.
- **`json()`** (`pxp:json`, in `xquery_json.pas`):
  - It is deprecated upstream in favour of `parse-json` / `json-doc`.
  - It parses its string argument liberally.
  - If the string starts with `http://`, `https://` or `file://`, **it fetches that URL**.
- **Version pinning:** FMD2's `.gitmodules` names `3rd/internettools` (benibela/internettools), but the tree at `ad3a5b63` has no gitlink for it, so the engine version is **unpinned**. Builds use whatever internettools master the builder has. The README also says to drop FLRE/PUCU into InternetTools for the regex engine.

## 2. Usage census

### Method

- A Lua-aware extractor tokenised every `.`/`:` call to `XPath`, `XPathString`, `XPathStringAll`, `XPathHREFAll`, `XPathHREFTitleAll` and `XPathCount` across `lua/` (modules, templates, extras, websitebypass and utils).
- It handles `'…'`, `"…"` and `[[…]]`, and skips commented-out lines.
- It took the first argument, inlining literals and marking `..`-concatenated Lua values as `$DYN`, and recorded the remaining arguments.
- Features were then counted per call site with regexes over the expression, with string literals masked.
- Counts are **call sites containing the feature**, not occurrences. They are regex-based, so expect ±a few.
- The scripts are not committed. They are trivial to re-run.

### Call sites

| | Count |
|---|---|
| Call sites | **3,607** in **347 files** (modules 3,058; templates 546; websitebypass 2; extras 1) |
| `CreateTXQuery(` calls | 1,159 (1,091 of them on `HTTP.Document`) |
| `XPathString` | 2,242 |
| `XPathStringAll` | 569 (340 with the default `', '` separator, 212 into `TStrings`, 17 with a custom separator) |
| `XPath` | 532 |
| `XPathHREFAll` | 196 |
| `XPathHREFTitleAll` | 51 |
| `XPathCount` | 17 |
| Fully literal expression | 3,523 |
| Literal + Lua concatenation | 71 |
| Wholly dynamic expression | 13 (e.g. an expression held in a module variable) |
| Calls passing an explicit context item | 752 (`XPathString` 701, `XPath` 48, `XPathCount` 2, `XPathHREFAll` 1) |

On the Lua side, modules also use these IXQValue methods and the `ParseHTML` method:

| Method | Uses |
|---|---|
| `ToString` | 627 |
| `Get` | 467 |
| `GetProperty` | 336 (JSON object property from Lua) |
| `GetAttribute` | 159 |
| `ParseHTML` | 130 (often re-parsing an extracted `<script>` body or JSON string) |
| `InnerHTML` / `OuterHTML` / `InnerText` | 0 |

### XPath path features

| Feature | Call sites |
|---|---|
| `//` | 2,339 |
| Predicates `[…]` | 2,322 |
| `@attr` | 2,002 |
| Context item `.` (e.g. `contains(., "x")`) | 479 |
| `contains(@class, …)` | 384 |
| Numeric predicate `[n]` | 323 |
| Function as a path step, `…/substring-after(., "x")` (XPath 2.0+) | 252 |
| `text()` kind test | 192 |
| `following-sibling::` | 107 |
| `[last()]` | 76 |
| `[last()-n]` | 68 |
| Arithmetic `+`/`-` (mostly inside `last()-1`) | 69 |
| Union `\|` | 20 |
| `self::` | 15 |
| `position()` | 4 |
| `parent::` | 3 |
| `preceding-sibling::` | 2 |
| `ancestor::` | 1 |
| `..` | 2 |
| General comparison with a sequence, e.g. `li[div=("Writer:", "Автор:")]` | present (templates) |

### XPath 3 operators

| Operator | Call sites |
|---|---|
| Simple map `!` | 10 |
| String concat `\|\|` | 8 |
| `=>`, `eq`/`lt`/…, `idiv`/`div`/`mod` as operators | 0 |

### Not used at all (0 call sites)

- `for`, `let`, `where` or `order by` (FLWOR)
- `some` / `every`
- `if … then … else`
- `$variables` and `:=`
- `map { }`, `array { }`, `[ ]` array constructors, JSONiq `{ }` object constructors
- `instance of`, `cast`, `treat`
- inline functions
- XQuery comments
- extended strings `x"…"`
- `count()`, `number()`, `distinct-values()`
- `matches()`, `extract()`

### Functions

| Function | Call sites | Standard? |
|---|---|---|
| `contains` | 739 | XPath 1.0 |
| `json` | 546 | **Internet Tools `pxp:json`** (deprecated upstream) |
| `last` | 146 | 1.0 |
| `string-join` | 133 | 2.0 |
| `substring-after` | 86 | 1.0 |
| `not` | 58 | 1.0 |
| `substring-before` | 36 | 1.0 |
| `starts-with` | 32 | 1.0 |
| `normalize-space` | 29 | 1.0 |
| `parse-json` | 18 | 3.1 |
| `css` | 16 (in 6 files) | **Internet Tools** (CSS3 selector; one uses `:nth-last-child(2)`) |
| `upper-case`, `lower-case` | 16 each | 2.0 |
| `substring` | 16 | 1.0 |
| `concat` | 15 | 1.0 |
| `replace` | 8 | 2.0 (regex) |
| `jn:members` | 6 | **JSONiq** |
| `position` | 4 | 1.0 |
| `jn:keys` | 3 | **JSONiq** |
| `name` | 3 | 1.0 |
| `resolve-uri` | 2 | 2.0 |
| `string-length` | 1 | 1.0 |
| `tokenize` | 1 | 2.0 (regex) |

That is **22 distinct functions** in all. 2 of them are Internet Tools-only (`json`, `css`) and 2 are JSONiq (`jn:keys`, `jn:members`).

### JSON navigation

This is the Internet Tools-specific part.

| Feature | Call sites | Example |
|---|---|---|
| Any of `json(` / `parse-json` / `jn:` | 572 | |
| — `json(*)` (parse the whole document text) | 510 | |
| — `json(<path>)` (parse a `<script>` substring) | 34 | |
| Dot property access `.prop` (PXP "unambiguous dot notation") | 482 | `json(*).results.total_page` |
| `()` array unboxing (JSONiq), any form | 334 | |
| — of which `.prop()` | 263 | `json(*).data()` |
| — other, e.g. `json(*)()`, `json(…)()/concat(…)` | | |
| XPath 3.1 lookup `?` | 167 | `genres?*?name`, `parse-json(.)?data` |
| Bare property name against an object context item | ~357 | `x.XPathString('title', v)` |
| Property via function call | 7 | `json(*)("pageCount")`, `(groups)(key)` |
| `/` child step over objects | ~10 | `json(*).data.episodes()[lockData/state="100"]`, `json(*).contents(1)/concat("/manga/",id,"/",slug)` |
| JSON literals `true` / `false` / `null` | 2 | `chapters?*[not(isUnreleased=true)]` |
| Dot names containing hyphens | ≥1 | `.data-second()` |

- **Bare property names:** a bare name against an object context relies on `jsonPXPExtensions`, which treats the child axis on an object as a property read. These are the `XPathString` calls with a single-name expression, almost all inside `for v in x.XPath('json(*).data()').Get() do … end` loops.
- **Dot names with hyphens:** Internet Tools resolves the ambiguity with `-` here; a port must match its rule.
- **Weak typing:** predicates compare JSON numbers or strings loosely, e.g. `jn:members(result)[Number=` .. n .. `]`, and objects or arrays coerce to strings or booleans (`AllowJSONiqOperations`). An engine with strict XPath 3.1 typing would raise errors here where Internet Tools quietly succeeds.

### By file

| | Files |
|---|---|
| Use JSON at all | 144 / 347 |
| Use dot navigation | 118 |
| Use `()` unboxing | 143 |
| Use `?` lookup | 44 |
| Use function steps | 79 |
| Use `!` or `\|\|` | 13 |
| Use `css()` | 6 |
| Use none of JSON, `css`, `!`/`\|\|` or function steps (XPath 1.0 paths plus a few 2.0 string functions at most) | 137 |

So an XPath 1.0 engine on its own would run at most ~39% of module files, and fewer still once `string-join` and similar 2.0 functions are counted.

### HTML parser leniency

- Modules query pages as delivered by real sites: tag soup, missing end tags, inline `<script>` JSON. They also query **non-HTML text** run through the same `pmHTML` parser:
  - `json(*)` takes the string value of the root element of a document that was really JSON.
  - 46 sites first wrap the body with `HTMLEncode(HTTP.Document.ToString())` so the parser won't eat `<`.
- **tbody:** 15 expressions spell out `table/tbody/tr` and 3 use `table//tr`. **None use `table/tr`**, so html5ever's always-insert-`tbody` behaviour breaks no expression in the census. Other WHATWG versus `pmHTML` differences still need testing on real pages: foster parenting, `<p>` auto-closing, nested `<a>` and `<form>`, and whitespace text nodes (`trimText=False`).
- Encoding detection is off (`autoDetectHTMLEncoding := False`), so the host must hand the parser correctly decoded text.

## 3. Options

What every option must cover:

- the path and function slice above
- PXP/JSONiq JSON navigation
- `json()` and `css()`
- a lenient HTML tree
- swallow-all-errors wrapper semantics
- IXQValue objects that can be iterated and used as context
- weak typing

### (a) Rust crates

**xee** ([Paligo/xee](https://github.com/Paligo/xee))

- **Status:**
  - xee-xpath 0.1.5 (2025-08-21). Last commit 2026-05-19. MIT.
  - Claims "almost complete XPath 3.1", passing 20,130 of 21,859 applicable QT3 tests.
  - `parse-json` is "mostly done"; `json-doc` and `json-to-xml` are not implemented ([`conformance/fn-todo.md`](https://github.com/Paligo/xee)).
- **Gaps:**
  - XML input only, via `xot`. HTML is an open question ([issue #105](https://github.com/Paligo/xee/issues/105)); there is an unmerged html5ever-to-xot fork.
  - No public extension-function API. The README says it "needs to be created", [issue #140](https://github.com/Paligo/xee/issues/140) is open, and PR #150 was closed without merging on 2026-05-14.
- **Coverage:** everything standard, but none of the JSONiq/PXP navigation, which is 144 files. It would need an expression-rewriting front end that turns `.p` into `?p`, `()` into `?*`, and bare names on objects into lookups. That rewrite needs type information that is only known at runtime: is the context an object or a node?
- **Effort:** high. That means a fork for extension functions (`json`, `css`, `jn:*`), an HTML-to-xot bridge, and a rewriting layer.
- **Risk:** medium-high. Strict typing will diverge from Internet Tools' weak typing, and upstream releases are slow.

**Skyscraper** ([James-LG/Skyscraper](https://github.com/James-LG/Skyscraper))

- **Status:** 0.7.0 (2026-05-03), MIT. It has its own WHATWG HTML parser and an XPath 3.1 subset with most axes, `!`, `||`, `=>`, FLWOR, maps/arrays and 100+ functions.
- **Gaps:**
  - No `parse-json` or `json-doc`.
  - No extension-function API.
  - API still major-version 0 ([`docs/features-backlog.md`](https://github.com/James-LG/Skyscraper/blob/main/docs/features-backlog.md)).
- **Coverage:** the HTML side of the census is likely fine. JSON is entirely missing.
- **Effort:** medium as a **fork base**: add JSON item types, PXP dot and child-on-object semantics, `json()`, `css()` and `jn:*`.
- **Risk:** medium. It is a small project with one maintainer, and FMD2r would own the fork.

**oxixml** ([cool-japan/oxixml](https://github.com/cool-japan/oxixml))

- **Status:** 0.1.2 (2026-08-10), Apache-2.0. The repo is only from 2026-07. It claims HTML5, XPath 3.1 with full QT3, and XQuery 3.1.
- **Assessment:** unproven, and it has no JSONiq. Worth at most a one-hour spike. High risk.

**sxd-xpath + sxd_html**

- XPath 1.0 only. Last release 2018 (0.4.2). It does support custom functions.
- Covers ≤137 files. Not viable.

**libxml crate** ([KWARC/rust-libxml](https://github.com/KWARC/rust-libxml))

- 0.3.22 (2026-10-07), MIT. It brings libxml2's lenient HTML parser and XPath 1.0.
- Custom functions only through raw unsafe bindings. Not thread-safe.
- Not viable as the engine; see (c).

**scraper / html5ever**

- scraper 0.27.0 is CSS-only. html5ever 0.40.1 (2026-09-14) is the standard WHATWG parser.
- Useful as the **parser** under option (d), and `scraper`'s `selectors` crate could implement `css()`.

**Others**

- xrust 2.2.0 (Apache-2.0, moved to GNOME GitLab): completeness unknown.
- uppsala and oxml: XPath 1.0 or early stage.
- amxml and xpath_reader: dead since 2018/19.

### (b) Internet Tools as a Free Pascal shared library behind C FFI

- **Feasibility:** technically straightforward.
  - Write a Pascal `library` that exports `cdecl` functions, e.g. `xq_create`, `xq_parse_html`, `xq_eval(expr, ctx)`, iteration, `xq_get_property`, `xq_get_attribute` and `xq_free`.
  - Exchange strings or JSON across the boundary.
  - Catch all exceptions on the Pascal side, which the wrapper already does.
  - Keep one engine per thread: Internet Tools raises "A TXQueryEngine must be destroyed in the thread that created it".
- **Platform:** FPC 3.2.x supports x86_64 and aarch64 Linux, so a multi-arch Docker build needs an FPC stage.
- **No existing library build:**
  - Xidel ships only as a CLI binary. The last stable release is 0.9.8 (2018); 0.9.9 exists only as nightlies.
  - There is no `library` project, no C API and no third-party bindings ([benibela/xidel](https://github.com/benibela/xidel), [videlibri.de/xidel.html](https://www.videlibri.de/xidel.html)).
- **Coverage:** ~100% by construction. It is the same engine and the same dialect, including XPath 4.0 parsing, PXP dot notation and weak typing.
- **Licence (blocker):**
  - `data/xquery.pas` is "GPL … version 3 … or (at your option) any later version", and `internettools.lpk` declares `GPLv3`.
  - Only some utility units (`bbutils.pas`) carry the LGPL-with-static-linking exception, and that does not cover the engine.
  - FMD2r's `LICENSE` is the GPL-2.0 text, and nothing in the repo grants "or later". GPLv3 code cannot be combined into a GPLv2-only work.
  - FMD2 itself links this engine, which suggests FMD2's effective terms are v2-or-later. That is not stated anywhere I found: FMD2 `licenses/` lists only GPL-2.0, and its README says "released under the GPLv2 license".
  - This needs an explicit licensing decision: either FMD2r is "GPL-2.0-or-later", so the binary is distributed under GPLv3, or (b) is out.
- **Effort:** low-moderate, about 1–2 weeks for the shim, FFI bindings and build pipeline, plus ongoing upkeep of a Pascal toolchain in CI.
- **Risk:**
  - Licence, as above.
  - Pinning: the engine is unpinned upstream, so FMD2r must pick an internettools commit.
  - Toolchain and FFI memory/exception hygiene.
  - It contradicts the "no Pascal" standing decision.
- A variant is **subprocessing the `xidel` CLI** per query. It gives the same fidelity with no FFI, but the latency is far too high for 3,600 call sites in hot loops, and the same GPLv3 question applies if the binary is shipped. It is only useful as a **test oracle**.

### (c) Other engines

**SaxonC-HE** ([saxonica.com/saxon-c](https://www.saxonica.com/saxon-c/index.xml))

- **Status:**
  - SaxonC 13, a GraalVM native image. MPL-2.0. XPath 3.1, XQuery 3.1 and XSLT 3.0.
  - Linux x86_64 and aarch64. `parse-json` and `json-doc` are in HE.
- **Gaps:**
  - **No HTML parsing in HE.** `fn:parse-html` and `saxon:parse-html` are PE/EE only ([docs](https://www.saxonica.com/html/documentation13/functions/fn/parse-html.html)).
  - No Rust bindings.
  - A heavy native library of tens of MB (unverified).
  - No JSONiq or PXP navigation.
- **Verdict:** same rewriting problem as xee, plus an html5ever-to-XML serialisation step, plus a large C dependency. Low fit.

**libxml2** ([GNOME/libxml2](https://gitlab.gnome.org/GNOME/libxml2))

- **Status:**
  - MIT. v2.15.4 (2026-09-01).
  - The maintainer stepped down on 2025-09-15.
  - The README advises against using it on **untrusted data**.
  - HTML5 tokenizer since 2.14, but not HTML5 tree construction.
- **Verdict:** XPath 1.0 only. It misses function steps (252 sites), `string-join` (133), all 2.0 string functions and all JSON (572). Not viable.

### (d) Purpose-built subset in Rust

**Scope**

- **Parsing:** html5ever builds into an FMD2r-owned arena DOM; `serde_json` (or a liberal parser that accepts trailing commas) handles JSON. Nodes, JSON objects and arrays, and atomics share one item model.
- **Expression parser and evaluator** covering exactly the census:
  - paths with the 7 axes in use, `//`, `@`, `.`, `..`
  - predicates, including numeric, `last()`, `last()-n` and `position()`
  - unions
  - general comparisons with sequences, `and`/`or`, `+`/`-`
  - function-call steps, `!` and `||`
  - parenthesised sequences
  - the 22 functions
  - PXP dot notation, `()` unboxing, function-call property access, `?` / `?*` lookup, child-step-on-object and JSON literals
  - Internet Tools' weak typing and atomisation rules
- **`css()`:** via the `selectors` crate, or by translating CSS to the internal AST.
- **Regex functions:** `replace` and `tokenize` via `regex` / `fancy-regex`, or [`regexml`](https://crates.io/crates/regexml) for XPath-compatible regex.
- **Wrapper semantics:** errors become the empty sequence, plus the trim and join rules.
- **Unknown syntax:** fail loudly in logs ("unsupported XPath feature …") while still returning empty to the module. This mirrors the "needs newer FMD2r" policy and turns gaps into actionable reports.

**Coverage, effort and risk**

- **Coverage:** 100% of the census by design. New upstream modules may use features outside it, so treat the feature table as a living contract.
- **Effort:** medium. That means a parser and evaluator for a deliberately small grammar, perhaps 3–5k lines of Rust, plus the DOM and JSON item model. A Skyscraper fork could provide the grammar and AST and cut the parser work.
- **Risk:** medium.
  - Semantic drift from Internet Tools in tree shape, whitespace text nodes, string values of objects, and the boundaries of dot notation.
  - Mitigate with **differential testing**: run the extracted corpus of 3,607 expressions against saved real pages through both FMD2r's engine and Xidel / Internet Tools, and diff the results. Xidel used only as a dev or CI oracle is not distributed with FMD2r, which avoids the GPLv3 question.

### Comparison

| Option | Coverage of census | Effort | Risk | Native deps |
|---|---|---|---|---|
| (a) xee + HTML bridge + extension-function fork + JSONiq rewriter | ~60% as-is, 100% only with a runtime-typed rewriter | High | Med-high | None |
| (a) Skyscraper fork + JSON + PXP extensions | HTML side ~95%; JSON 0% until added | Medium | Medium | None |
| (a) sxd-xpath / libxml crate | ≤39% of files | — | — | libxml2 (C) |
| (b) Internet Tools FPC `.so` | ~100% | Low-med | **Licence blocker**, Pascal toolchain | FPC-built `.so` |
| (c) SaxonC-HE | ~60%, no HTML, no JSONiq | High | Med-high | Large GraalVM `.so` |
| (c) libxml2 | ≤39% of files | Low | High (untrusted input) | libxml2 |
| (d) Purpose-built subset | 100% of the census | Medium | Medium (drift, so a test oracle is needed) | None |

## 4. Implications for other tickets

- **The Pascal exception in #1 can be dropped** if (d) is chosen. If (b) is wanted anyway, the project licence must first be settled as "GPL-2.0-or-later"; that is a new decision for the map.
- **The module compatibility-testing ticket should plan a differential XPath oracle** (Xidel or Internet Tools, dev-only) and a page-fixture corpus. The 3,607-expression census extractor doubles as the input set.
- **Host API surface:** `CreateTXQuery` plus the IXQValue methods (`Get`, `GetProperty`, `GetAttribute`, `ToString`, `Count`, and the unused `InnerHTML`/`OuterHTML`/`InnerText`) belong in the Host API inventory. IXQValue values must work as context arguments.
- **`json("http…")` performs a network fetch inside XPath.** FMD2r's engine should either route it through the host HTTP client (cookies, bypass) or refuse it. No module in the census passes a URL, but the dialect allows it.
- **The upstream engine version is unpinned** (no gitlink for `3rd/internettools`), and `evaluateXPath` parses as **XPath 4.0** at master. Any FMD2 build may behave slightly differently, so "what upstream does" means "what current internettools master does".

## Sources

- FMD2 `ad3a5b63`:
  - `baseunits/XQueryEngineHTML.pas`, `baseunits/lua/LuaXQuery.pas`, `baseunits/lua/LuaIXQValue.pas`
  - `.gitmodules`, `README.md`, `licenses/`
  - `lua/**` (census)
- Internet Tools, master (pushed 2026-07-01):
  - [`data/xquery.pas`](https://github.com/benibela/internettools/blob/master/data/xquery.pas): `TXQueryEngine.create`, `evaluateXPath`, `TXQStaticContext` docs, licence header
  - [`data/xquery_json.pas`](https://github.com/benibela/internettools/blob/master/data/xquery_json.pas): `xqFunctionJSON`, `jn:` registration
  - [`data/simplehtmltreeparser.pas`](https://github.com/benibela/internettools/blob/master/data/simplehtmltreeparser.pas): `TParsingModel`
- Xidel: https://github.com/benibela/xidel, https://www.videlibri.de/xidel.html
- xee: https://github.com/Paligo/xee (README, `conformance/fn.md`, `conformance/fn-todo.md`, issues #105, #140, PR #150); https://crates.io/crates/xee-xpath
- Skyscraper: https://github.com/James-LG/Skyscraper (README, `docs/features-backlog.md`)
- oxixml: https://github.com/cool-japan/oxixml
- sxd-xpath: https://github.com/shepmaster/sxd-xpath; sxd_html: https://crates.io/crates/sxd_html
- rust-libxml: https://github.com/KWARC/rust-libxml
- html5ever: https://crates.io/crates/html5ever; scraper: https://crates.io/crates/scraper; regexml: https://crates.io/crates/regexml
- SaxonC: https://www.saxonica.com/saxon-c/index.xml, https://www.saxonica.com/download/c.xml, https://www.saxonica.com/html/documentation13/functions/fn/parse-html.html
- libxml2: https://gitlab.gnome.org/GNOME/libxml2 (README, NEWS), https://discourse.gnome.org/t/stepping-down-as-libxml2-maintainer/31398
