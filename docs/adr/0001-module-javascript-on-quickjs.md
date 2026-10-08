# Run module JavaScript on QuickJS, not Duktape

Upstream evaluates `fmd.duktape.ExecJS` on Duktape 2.x. FMD2r uses QuickJS-NG through rquickjs instead, because it is maintained, has a Rust API, and supports per-call timeout and memory limits. The Duktape-specific behaviour modules rely on is reproduced in the host: a CommonJS `require` shim (for `crypto-js.min.js`), global-script completion values, and Duktape's result-to-string conversion. Snapshot tests on every upstream `ExecJS` call site pin that behaviour.

## Considered Options

- **Vendored `duktape.c` 2.7**: matches upstream by construction, but the engine is effectively unmaintained, needs hand-written FFI, and has no built-in exec timeout. It remains the fallback if a divergence that can't be shimmed ever appears.
- **boa**: pure Rust, but less mature, and it would add a third engine's quirks on top of the Duktape behaviour we already have to emulate.

## Consequences

Each call gets a fresh runtime with a 10 s / 64 MB limit. A script that exceeds either fails as a module error instead of blocking a worker, which upstream cannot do.
