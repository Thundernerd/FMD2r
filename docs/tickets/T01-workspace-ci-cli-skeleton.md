# T01: Cargo workspace, crate stubs, CI (fmt, clippy, test), dev CLI skeleton
Deps: none

## Goal
Create the Cargo workspace laid out in `docs/plan.md` ("Architecture") so that every later ticket has a crate to land in, a CI pipeline that enforces `CODING_STANDARDS.md`, and a `fmd2r` binary with the command tree that later tickets fill in.

## Scope (in/out)
In:
- Root `Cargo.toml` workspace (edition 2021 or later, shared `[workspace.dependencies]`, `[workspace.lints]` if useful) with member crates under `crates/`: `fmd-lua`, `fmd-xpath`, `fmd-http`, `fmd-store`, `fmd-core`, `fmd-pack`, `fmd-import`, `fmd-server`, `fmd2r` (binary). Each library is a stub `lib.rs` with a crate-level doc comment stating its responsibility (copy from the plan).
- Empty placeholder directories `crates/xpath-fpc/` (README only; T07 fills it) and `web/` (README only; T22 fills it).
- `rust-toolchain.toml` pinning a stable toolchain with `rustfmt` and `clippy`.
- `fmd2r` binary using `clap` (derive) with subcommands: `serve`, `module init|info|pages|download`, `xpath eval`. Each unimplemented subcommand prints a clear "not implemented yet (Tnn)" message to stderr and exits with a non-zero code; `--help` and `--version` work.
- GitHub Actions workflow `.github/workflows/ci.yml`: on push and PR, run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Cache cargo artifacts.
- `.gitignore` additions for `target/`, `web/node_modules`, `web/build`, local fixture/userdata dirs.

Out: any real functionality in the library crates; the FPC build (T07); the frontend build (T22); Docker (T33).

## Seams under test
- The `fmd2r` binary's CLI, run as a process (e.g. `assert_cmd`): `fmd2r --help` lists `serve`, `module`, `xpath`; `fmd2r --version` prints the crate version; `fmd2r module init` exits non-zero with a "not implemented" message.

## Acceptance criteria
- [ ] `cargo build --workspace`, `cargo test --workspace`, `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` all pass locally.
- [ ] The CI workflow runs those three checks on PRs and passes on this branch.
- [ ] All crates from the plan exist with the names above and a doc comment describing their role.
- [ ] `fmd2r --help` shows the subcommand tree; unimplemented subcommands fail cleanly (no panic).
- [ ] The binary depends on `anyhow`; library crates do not.

## FMD2 references
- `mangadownloader/md.lpi` (FMD2 project file; shows the unit layout being replaced)
- `baseunits/FMDOptions.pas:259-300` (directory layout FMD2 uses: userdata, `lua/`, `lua/modules/`; for orientation only)
