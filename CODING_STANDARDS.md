# Coding standards

These apply to every ticket in `docs/tickets/`. `/code-review` checks changes against this file.

## Rust

- **Formatting:** `cargo fmt --all` with the default rustfmt config. CI runs `cargo fmt --all -- --check`.
- **Lints:** `cargo clippy --workspace --all-targets -- -D warnings` must pass. Don't silence a lint with `#[allow]` unless a comment says why.
- **Errors:** library crates (`fmd-*`) define their error types with `thiserror`. Only the `fmd2r` binary uses `anyhow`.
- **No panics in production code:** no `unwrap()`, `expect()`, `panic!`, `todo!` or `unimplemented!` outside `#[cfg(test)]` code and `tests/`. Propagate errors with `?` instead. A Lua callback that fails becomes a Lua error, never a Rust panic.
- **FMD2 provenance:** every Host API behaviour (anything a Lua module can observe) has a doc comment citing the Pascal (or Lua) source it reproduces, as a `file:line` path relative to the FMD2 checkout, e.g.
  `/// Returns true when the body is non-empty, even on a 404 (baseunits/httpsendthread.pas:592).`

## Tests

- Work test-first (`/tdd`), in vertical red→green slices.
- Test **only at the seams declared in the ticket** under "Seams under test": a Lua snippet run against the public `fmd-lua` runtime API, a public crate API, or an HTTP handler. Don't test private functions or internal modules directly.
- Expected values come from FMD2's behaviour (the cited Pascal source), not from the Rust implementation.
- Tests never reach the network. Use mocked HTTP or recorded fixtures.

## Frontend (`web/`)

- SvelteKit with Svelte 5 (runes) and TypeScript in `strict` mode. No `any` without a comment explaining why.
- The API client is generated from the server's OpenAPI document; don't hand-write request types.
