#!/bin/sh
# Sets the workspace version in Cargo.toml and the workspace crates' Cargo.lock entries (so
# `--locked` builds still pass). The release workflow runs it so builds report the tag's version.
#
# Usage: scripts/set-version.sh VERSION   (e.g. 1.2.3 or 1.2.3-rc.1; a leading `v` is dropped)
set -eu
fail() {
  echo "FAIL: $*" >&2
  exit 1
}
[ $# -eq 1 ] || fail "usage: scripts/set-version.sh VERSION"
version=${1#v}
echo "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$' ||
  fail "not a semver version: $1"
root=$(cd "$(dirname "$0")/.." && pwd)

# The crates that take the workspace version (crates/xpath-fpc is outside the workspace).
members=$(for manifest in "$root"/crates/*/Cargo.toml; do
  if grep -q '^version\.workspace = true' "$manifest"; then
    sed -n 's/^name = "\(.*\)"$/\1/p' "$manifest" | head -n1
  fi
done)
[ -n "$members" ] || fail "no crate uses the workspace version"

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

awk -v version="$version" '
  /^\[/ { section = $0 }
  section == "[workspace.package]" && /^version = / { $0 = "version = \"" version "\""; n++ }
  { print }
  END { if (n != 1) exit 1 }
' "$root/Cargo.toml" >"$tmp" || fail "Cargo.toml has no [workspace.package] version"
cat "$tmp" >"$root/Cargo.toml"

# In Cargo.lock each [[package]] starts with its name, then its version.
expected=$(echo "$members" | wc -l)
awk -v version="$version" -v members="$members" -v expected="$expected" '
  BEGIN { split(members, list, "\n"); for (i in list) member[list[i]] = 1 }
  /^name = / { name = $3; gsub(/"/, "", name) }
  /^version = / && (name in member) { $0 = "version = \"" version "\""; n++; name = "" }
  { print }
  END { if (n != expected) exit 1 }
' "$root/Cargo.lock" >"$tmp" || fail "Cargo.lock lacks some workspace crates ($expected expected)"
cat "$tmp" >"$root/Cargo.lock"

echo "version set to $version for:" $members
