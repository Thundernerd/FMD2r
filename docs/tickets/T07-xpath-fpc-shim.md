# T07: XPath FPC shim (`xpath-fpc`)
Deps: T01

## Goal
Build a small Free Pascal shared library, `libfmdxpath.so`, that exports FMD2's own XPath/XQuery engine (Benito van der Zander's *internettools*, as configured by `XQueryEngineHTML.pas`) over a plain C ABI, so Rust can evaluate module XPath with **bit-identical** behaviour to FMD2. Built by `build.rs` and in CI.

## Scope (in/out)
In:
- `crates/xpath-fpc/`: an FPC library project (`.lpr`/`.pas`, buildable with `fpc` alone, no Lazarus IDE) that vendors or fetches internettools at a pinned commit (FMD2 links the `internettools` Lazarus package; source at https://github.com/benibela/internettools — pin the revision FMD2 uses if determinable, else latest compatible, and record it).
- Engine configuration copied from `TXQueryEngineHTML.Create`: `parsingModel := pmHTML`, repair missing start/end tags, `trimText := False`, `readComments := False`, `readProcessingInstructions := False`, `autoDetectHTMLEncoding := False`; evaluation errors yield an empty value instead of raising.
- C ABI (all functions `cdecl`, no exceptions crossing the boundary, UTF-8 byte buffers with explicit lengths):
  - `fx_doc_parse(const char* html, size_t len) -> fx_doc*` / `fx_doc_free`
  - `fx_eval(fx_doc*, const char* expr, size_t len, fx_value* context_or_null, int is_css) -> fx_value*` (never NULL; empty sequence on error, with `fx_last_error` retrievable)
  - value handles: `fx_value_free`, `fx_value_count`, `fx_value_get(v, i)` (1-based), `fx_value_kind`, `fx_value_to_string`, `fx_value_inner_html`, `fx_value_outer_html`, `fx_value_inner_text`, `fx_value_get_attribute(v, name)`, `fx_value_get_property(v, name)` (JSON object property)
  - strings returned via `fx_string { const char* ptr; size_t len; }` freed with `fx_string_free`.
  - Thread safety: each call is safe from any thread as long as a given doc/value is not used concurrently; document any global state in internettools and guard it.
- A C header `fmdxpath.h` checked in.
- A minimal C (or Rust `#[test]` with raw `extern "C"`) smoke test proving the ABI loads and evaluates.
- CI job that installs FPC (e.g. `apt install fpc`), builds the `.so`, and uploads it as an artifact for T08.

Out: the Rust trait and Lua bindings (T08); Windows/macOS builds (later).

## Seams under test
- The C ABI itself, called from a Rust integration test in this crate (or `fmd-xpath`'s `build.rs` smoke) through raw FFI:
  - `parse("<ul><li>a</li><li> b </li></ul>")`, `eval("//li")` → count 2; `get(…,2)` → `to_string == " b "` (trimText false).
  - `eval("//li[")` → empty value, `fx_last_error` non-empty, no crash.
  - `eval("json('{\"a\":[1,2]}')?a?*")` → count 2 (Xidel JSON extensions work).
  - `eval("div.x", is_css=1)` against `<div class=x>t</div>` → `t`.
  - Malformed HTML (`<p>a<p>b`) is repaired: `count(//p) == 2`.

## Acceptance criteria
- [ ] `libfmdxpath.so` builds from a clean checkout with only `fpc` installed, via a documented command and in CI.
- [ ] The header and the exported symbols match; no Pascal exception can escape across the ABI.
- [ ] Parser configuration matches `XQueryEngineHTML.pas:384-400` line by line (cite it in the source).
- [ ] Memory: every handle has a free function; a test runs parse/eval/free in a loop without growth (rough check).
- [ ] Pinned internettools revision recorded in `crates/xpath-fpc/README.md`.

## FMD2 references
- `baseunits/XQueryEngineHTML.pas:384-415` (engine and tree parser configuration, stream constructor)
- `baseunits/XQueryEngineHTML.pas:236-330` (`Eval`, `EvalString`, `EvalCount` incl. CSS variants and error handling)
- `baseunits/XQueryEngineHTML.pas:332-382`, `:462-545` (`XPathStringAll`, `XPathHREFAll`, `XPathHREFtitleAll` helpers T08 builds on)
- `mangadownloader/md.lpi:348` (`internettools` package dependency)
- `docs/LUA-REFERENCE.md:993-1056` (TXQuery methods and XPath extensions modules use: `json()`, `?*`, `jn:*`, `css()`)
