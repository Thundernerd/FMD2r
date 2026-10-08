# fmd-xpath

The `XPathEngine` trait FMD2r's Lua bindings (`CreateTXQuery`, `IXQValue`) run on, and its two backends:

| Backend | Cargo feature | What it is |
|---|---|---|
| `fpc` | `fpc` (off by default; needs `fpc`, see `crates/xpath-fpc/README.md`) | FMD2's own engine, internettools, over the C ABI of `libfmdxpath.so`. Bit-for-bit FMD2. |
| `native` | `native` (on by default) | Pure Rust: html5ever builds the tree, and an evaluator written for this crate implements XPath 3.1 with internettools' semantics and extensions. No FPC dependency. |

`fmd_xpath::Backend` names them; `Backend::engine()` returns `None` for a backend the build leaves out. `fmd-lua`'s
`Runtime::set_xpath_backend` installs `CreateTXQuery` over one, and the `xpath.backend` setting (`fpc` | `native`,
default `fpc`) picks it. T35 switches the default once the differential corpus shows parity.

## Tests

`tests/shared/mod.rs` is one suite run against every backend (`tests/fpc.rs`, `tests/native.rs`): T08's cases plus
the extension cases inventoried in `docs/xpath-extensions.md`. Its expected values are FMD2's: a case passes on `fpc`
first, then `native` must match.

```sh
cargo test -p fmd-xpath                     # native only
cargo test -p fmd-xpath --features fpc      # both backends
```

`fmd-lua`'s XQuery suite (`crates/fmd-lua/tests/xquery/`) does the same through Lua snippets.

## Why not xee

The ticket suggested `xee` for XPath 3.1. Modules depend on internettools behaviour that is not XPath 3.1 and that
xee's typed, standards-strict evaluator has no hooks for: dot notation (`json(*).data.items()`), calling arrays and
objects with no arguments, child and descendant steps on JSON objects, the case-insensitive "clever" default
collation, lenient typing (a sequence passed to a string parameter is concatenated; `'10' = 10` is true), node
strings trimmed on every conversion, JSON values that print like internettools' (`json('[1,2]')` is `12`), and
evaluation errors turned into empty values. Rewriting expressions for xee would still need a full parser for the
extended grammar, and the results would still have to be mapped back to internettools' value model. The `native`
evaluator implements that model directly (`src/native/`):

- `dom.rs`: html5ever's tree, reshaped as internettools builds it (no comments, leading whitespace kept, scripting
  off, no quirks mode), plus internettools' HTML serializer and `innerText`.
- `syntax.rs`, `eval.rs`, `value.rs`: the grammar, evaluation, and the value model with internettools' conversions
  and comparisons (`compareCommon`, `case-insensitive-clever`).
- `functions.rs`, `json.rs`, `css.rs`: the function library, the liberal JSON parser, and the CSS-to-XPath translation.

Behaviour the native backend reproduces cites internettools as `internettools data/<file>:<line>`, relative to the
pinned revision `crates/xpath-fpc/build.sh` downloads (FMD2's checkout doesn't contain it).

## Known differences of the `native` backend

None of these show in the shared suite; they are what the T35 differential corpus is for.

- **HTML tree repair** follows HTML5 (html5ever), which internettools' `pmHTML` repair only approximates. Pages
  that are valid HTML parse the same; malformed markup may not. Seen so far:
  - `textarea` and `title` content is text, not markup (`<textarea><b>x</b></textarea>`).
  - `<div/>` is a start tag, not an empty element; an unquoted attribute value takes a trailing `/`
    (`<circle r=1/>` gives `r="1/"`).
  - Elements in the wrong place move or vanish: table rows or cells outside a table (`<tr><td>a</td></tr>` as a
    whole document keeps only the text), text inside `<table>` moves before it, `<td>` directly in `<table>` gets
    `<tbody><tr>`, a nested `<form>` or `<a>` is dropped or closed, `<h1><h2>` closes the `h1`, a stray `</p>`
    creates an empty `p`, `</ul>` closes an open `li`, `<template>` stays in `head`.
  - Misnested formatting elements are fixed up by the adoption agency algorithm (`<b><i></b></i>` gives two `i`).
  - Attributes of a second `<body>` are merged; duplicate attributes are dropped; `&copy=` in an attribute is not
    decoded.
  - `<![CDATA[...]]>` outside SVG and MathML is a comment, so dropped.
- **Names** are lowercased (HTML5), where internettools keeps the source's case for `name()` and serialization
  (`<DIV>`). Name tests are case-insensitive in both, so paths match the same nodes.
- **`resolve-uri` without a base URI** returns the relative URI unchanged; FMD2 resolves it against its own
  working directory (`file:///...`).
- **Decimals** have 28 significant digits (`rust_decimal`) instead of arbitrary precision.
- **Floating point**: `xs:double("1e400")` is `INF`; FMD2's engine gives `5.0E-324`.
- **Not implemented** (unused by the corpus, see `docs/xpath-extensions.md`): `x"..."` extended strings and
  backtick strings, `$var := value`, `extract`, `eval`, `form`, date/time types and functions, higher-order
  functions, `fn:sort`, XQuery. They yield an empty value.
- **JSON nesting** deeper than 512 levels is an error (an empty value), so hostile input can't exhaust the stack.
- **Performance**: tree building is quadratic in nesting depth (the HTML5 algorithm's scope checks); 10,000 nested
  elements take a few seconds in a debug build.
