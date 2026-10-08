# XPath extensions upstream modules use

An inventory of the internettools (Xidel) behaviour beyond plain XPath 3.1 that the upstream module corpus relies on,
for the `native` backend of `fmd-xpath` (T34). Each feature lists the shared-suite test that pins it: the suite
(`crates/fmd-xpath/tests/shared/mod.rs`) runs against both backends, so every expectation is FMD2's own engine's
answer (the `fpc` backend) and the `native` backend must match it.

## How it was gathered

The corpus is `fixtures/lua/modules` at upstream `ad3a5b632375a5712065373027b0e8cc98a56b60` (T02). Every string
literal passed as the first argument of `XPath`, `XPathString`, `XPathStringAll`, `XPathCount`, `XPathHREFAll` or
`XPathHREFTitleAll` was extracted: 3,594 calls, 2,738 distinct expressions, in 346 modules. Expressions built by
concatenation only contribute their first literal, so counts are lower bounds. Counts below are distinct expressions
and the modules they appear in.

A second pass took every string literal in the tree that looks like XPath (3,304, including expressions kept in
variables and Lua long strings) and listed the functions they call; it added `object()` and `jn:object()`
(`Cubari.lua`, `Guya.lua`). Both passes were also evaluated on both backends against sample HTML and JSON
documents, with and without a JSON context item, and gave the same results.

## Engine set-up the modules see

`TXQueryEngine.Create` (internettools `data/xquery.pas:8376-8414`) and FMD2's `TXQueryEngineHTML`
(`baseunits/XQueryEngineHTML.pas:384-400`) fix these, and they apply to every expression:

| Behaviour | Where it shows | Test |
|---|---|---|
| Node strings are trimmed of every character up to `' '` (`XQGlobalTrimNodes`) | everywhere a node becomes a string | `get_is_one_based_and_out_of_range_is_empty` |
| Default collation `case-insensitive-clever`: ASCII case-insensitive, digit runs compared as numbers | `=`, `<`, `contains` (663 expressions / 199 modules), `starts-with` (32/15), `substring-before/after` (89/70), `distinct-values`, `index-of` | `strings_join_and_compare_like_internettools` |
| No strict typing: a string parameter gets a sequence's strings concatenated; a string compared with or added to a number is converted to one | `contains(//li, ...)`, `'10' = 10`, `3 + '4'`, `\|\|` (8/8) | `strings_join_and_compare_like_internettools` |
| Integers divide to 18-digit decimals; doubles print in XPath 3.1 canonical form; integer division by zero fails, double division gives `INF` | arithmetic in modules (page counts) | `numbers_print_like_internettools` |
| Any evaluation error yields an empty value | every call | `an_invalid_expression_yields_an_empty_value` |

## Extensions

| Feature | Corpus use | Example | Test |
|---|---|---|---|
| `json($text)`: parses JSON text (a node's string, e.g. `json(*)`), liberally: single quotes, bare keys, trailing commas, several top-level values, JSONiq number types | 337 / 144 | `json(*).data.title` | `json_parses_text_and_nodes_into_objects_arrays_and_numbers` |
| `parse-json($text)` | 14 / 14 | `parse-json(.)?data?*` | `json_parses_text_and_nodes_into_objects_arrays_and_numbers` |
| Dot notation: `.name` reads a property after `)`, `]` or `}`; names may hold dots (`.data.items`); items without the property are skipped; `$var.name` is a variable name, not a lookup | 315 / 118 | `json(*).props.pageProps.chapters()` | `dot_notation_reads_properties_after_a_parenthesis` |
| Calling a JSON value: `$array()` lists members, `$object()` lists keys, `$object("key")`, `$array(n)` | `()` 199 / 139; `("key")` 5 / 4 | `json(*).genres().name` | `calling_objects_and_arrays_reads_them` |
| `?` lookups: `?key`, `?*`, `?n`, also right after a path step (`genres?*?name`); a lookup on a node fails | `?key` 101 / 33; `?*` 101 / 42; `?n` 1 / 1 | `string-join(genres?*?name, ', ')` | `lookups_read_objects_and_arrays` |
| Path steps on JSON (PXP extensions): `child::name` reads a property (of every object in an array), `//name` searches at any depth, `true`/`false`/`null` are literals | `?*/name` 8 / 5; `true` 2 / 2; plus any relative step with a JSON context item | `statuses?*/name`, `restricted_view?is_open=true` | `steps_on_objects_read_properties` |
| `jn:keys`, `jn:members` (and `jn:size`, `jn:null`, `jn:is-null`) | `jn:keys` 2 / 2; `jn:members` 3 / 3 | `jn:keys(json(*))` | `jn_functions_list_keys_and_members` |
| `object(("key", value, ...))` and `jn:object($objects)`, which merges objects (a repeated key is an error) | 2 / 2 (in long strings) | `jn:object(object(("chapter_id", $k)), (chapters)($k))` | `object_constructors_build_and_merge_objects` |
| `css($selector)`: CSS 3 selectors translated to XPath like `TXQueryEngine.parseCSSTerm` (data/xquery.pas:8749-9119), starting at the context node itself | 16 / 6 | `css("div.chapter-list > div.slot > a")` | `css_selects_like_internettools`, `a_context_value_scopes_xpath_and_css` |
| `string-join` with internettools' typing (atomized items, any separator) | 111 / 73 | `string-join(.//a, ", ")` | `strings_join_and_compare_like_internettools` |
| Simple map `!` and string concatenation `\|\|` (XPath 3.0) | `!` 10 / 8; `\|\|` 8 / 8 | `//script ! substring-after(., "x = ")` | `strings_join_and_compare_like_internettools` |

Other XPath 3.1 the corpus uses and the `native` backend implements: `text()` (157 / 95), `following-sibling::`
(78 / 44), `last()` (96 / 117), positional predicates (272 / 145), `normalize-space`, `upper-case`/`lower-case`,
`concat`, `replace`/`matches`/`tokenize`, `resolve-uri`, `position()`, `preceding-sibling::`.

## Not used by the corpus, not implemented

`x"..."` extended strings, `$var := value` assignments, `extract`, `eval`, `form`, date and time functions, and
higher-order functions. They yield an empty value (an error) on the `native` backend; see the crate README's known
differences.
