#!/bin/sh
# Smoke-tests a release tarball: it holds fmd2r, README.md and LICENSE but no libfmdxpath.so, and
# the binary runs `--version` and loads the bundled Lua modules with `module init`. The release
# workflow runs it on each tarball, the arm64 one under QEMU.
#
# Usage: scripts/tarball-smoke.sh TARBALL [LUA_DIR]   (default LUA_DIR: fixtures/lua)
# FMD2R_RUNNER prefixes each fmd2r command, e.g. FMD2R_RUNNER='qemu-aarch64 -L /usr/aarch64-linux-gnu'.
set -eu

tarball=$1
lua_dir=$(cd "${2:-fixtures/lua}" && pwd)
runner=${FMD2R_RUNNER:-}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

tar -C "$dir" -xzf "$tarball"
root=$dir/$(basename "$tarball" .tar.gz)
[ -d "$root" ] || fail "the tarball has no top-level directory $(basename "$root")"

echo "contents"
for f in fmd2r README.md LICENSE; do
  [ -f "$root/$f" ] || fail "the tarball has no $f"
done
lib=$(find "$dir" -name 'libfmdxpath*')
[ -z "$lib" ] || fail "the tarball contains $lib"

echo "fmd2r --version"
# shellcheck disable=SC2086 # the runner is a command line
$runner "$root/fmd2r" --version || fail "fmd2r --version"

echo "fmd2r module init loads the bundled modules"
# shellcheck disable=SC2086
$runner "$root/fmd2r" module init --lua-dir "$lua_dir" >/dev/null || fail "fmd2r module init"

echo "OK"
