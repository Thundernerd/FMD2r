# Test fixtures

## `lua/`: upstream Lua snapshot

A copy of the `lua/` tree (`modules`, `templates`, `utils`, `websitebypass`, `extras`) from
[dazedcat19/FMD2](https://github.com/dazedcat19/FMD2), the repository FMD2 itself updates its
modules from. `lua/UPSTREAM_REF` holds the commit it was taken from.

The snapshot is **committed**, not fetched in CI, so `cargo test` runs offline and every run
checks the same modules. Refresh it on purpose and commit the result:

```sh
scripts/sync-upstream-lua.sh               # latest master
scripts/sync-upstream-lua.sh --ref <sha>   # a specific commit, branch or tag
```

The script replaces `fixtures/lua` wholesale; don't edit files in it by hand.

Tests reach the snapshot through the `fmd-testkit` dev crate: `corpus_root()`, `module_files()`,
`check_each()` (runs a check on every module and reports all failures at once) and
`scan_host_api_names()` (the Host API names a Lua source references).
