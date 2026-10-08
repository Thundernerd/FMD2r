# The Host API absorbs upstream's Windows assumptions; FMD2r never patches upstream

Upstream modules and their helper scripts assume Windows. They use backslash paths, `cmd.exe /c … && …`, a `python` executable, and `cloudflare.py` resolving its folder via `Path(__file__)/'..'`, which only works on Windows. FMD2r runs them unmodified on Linux by translating at the Host API boundary, not by editing modules or waiting on upstream fixes:
- On non-Windows, every Host API file entry point and every subprocess argument rewrites `\` to `/`.
- A small, explicit command rule table maps `cmd.exe /c <args>` to `sh -c` and `python` to `python3`.
- Child processes inherit `/data` as their CWD and get quiet-npm env vars.
- An upstream helper script that can't run on Linux even after translation, such as `cloudflare.py`, gets a **Rust-native replacement** behind a command rule. The replacement speaks the script's CLI/stdout contract directly to FlareSolverr.

On Windows, every rule is a no-op.

## Considered Options

- **Upstream PRs first** (for example, fixing `cloudflare.py`'s path logic): rejected. FMD2r shouldn't depend on upstream's review queue or its interest in non-Windows platforms.
- **Per-case Lua shims** (FMD2r-maintained copies of `utils/nodejs.lua` and others): rejected, because they break "runs unmodified" and drift silently.
- **Filesystem tricks** (symlinks, backslash-named files): unworkable, because `\` is a legal filename character on Linux.

## Consequences

- Native replacements track a helper's CLI contract, not its code. When upstream changes such a script, the compatibility tests have to catch the drift.
- A module that ever needs a literal backslash in a path or command argument will break. The offline "runs cleanly" gate is where that shows up.
