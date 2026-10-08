# T35: XPath differential corpus and default-backend switch
Deps: T16, T34

## Goal
Prove the native XPath backend is equivalent to the FPC one on real module traffic, then make it the default.

## Scope (in/out)
In:
- Logging hook (from T08) enabled during smoke runs (T16, replay and nightly live): records `(expression, context description, document hash)` plus the document body (deduplicated by hash) into `fixtures/xpath-corpus/`.
- A differential runner (`cargo test -p fmd-xpath --features diff` or `fmd2r xpath diff`) that evaluates every corpus entry on both backends and compares normalised results (sequence length, item kinds, string values, serialised nodes).
- Report: list of mismatches grouped by expression feature; fixes go into T34's backend (in this ticket) until zero mismatches.
- When at full parity: switch the default `xpath.backend` to `native`, keep `fpc` selectable, update `docs/plan.md` decisions note, and make CI run the native backend by default (FPC job kept as a parity guard).
- `fmd2r xpath eval --backend fpc|native <file> <expr>` CLI for debugging.

Out: removing the FPC shim entirely (later decision).

## Seams under test
- The differential runner's public API: given a corpus dir with entries where both backends agree → pass; inject a fake backend returning a different result for one entry → reported mismatch naming the expression and doc hash.
- The `fmd2r xpath eval` CLI on a fixture file with both backends prints identical output.

## Acceptance criteria
- [ ] Corpus collected from all smoke entries (≥ 1000 unique expression/doc pairs, or all available).
- [ ] Zero mismatches on the corpus before the default flips.
- [ ] Default backend switched with the FPC backend still available.

## FMD2 references
- `baseunits/XQueryEngineHTML.pas` (reference semantics via the FPC backend)
- `baseunits/lua/LuaXQuery.pas`, `baseunits/lua/LuaIXQValue.pas`
