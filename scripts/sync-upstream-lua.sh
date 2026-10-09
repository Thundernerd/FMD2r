#!/usr/bin/env bash
# Refresh fixtures/lua from the upstream FMD2 Lua tree.
#
# FMD2 tracks its modules at dazedcat19/FMD2, ref master, path lua (dist/config.json "GitHub"
# block). Replaces the destination with that `lua/` tree at one commit and writes the SHA to
# UPSTREAM_REF. A shallow, sparse git fetch needs no GitHub API token.
#
# Usage: scripts/sync-upstream-lua.sh [--ref REF] [--repo URL] [--dest DIR]
#   --ref   branch, tag or commit to sync (default: master)
#   --repo  git URL of the upstream repository (default: https://github.com/dazedcat19/FMD2)
#   --dest  directory to populate (default: fixtures/lua in this repository)
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ref=master
repo=https://github.com/dazedcat19/FMD2
dest="$repo_root/fixtures/lua"
path=lua

usage() {
    sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
}

while (($#)); do
    case "$1" in
        --ref) ref=${2:?--ref needs a value}; shift 2 ;;
        --repo) repo=${2:?--repo needs a value}; shift 2 ;;
        --dest) dest=${2:?--dest needs a value}; shift 2 ;;
        -h | --help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

git_checkout() { git -C "$tmp/repo" -c advice.detachedHead=false "$@"; }

git init --quiet "$tmp/repo"
git_checkout remote add origin "$repo"
git_checkout sparse-checkout set --no-cone "/$path/"
git_checkout fetch --quiet --depth 1 --filter=blob:none origin "$ref"
git_checkout checkout --quiet FETCH_HEAD
sha=$(git_checkout rev-parse HEAD)

if [[ ! -d "$tmp/repo/$path/modules" ]]; then
    echo "$repo at $ref has no $path/modules directory" >&2
    exit 1
fi

# Stage next to the destination, then swap, so a failed copy never leaves a half-synced tree.
mkdir -p "$(dirname "$dest")"
staged=$(mktemp -d "$dest.sync.XXXXXX")
trap 'rm -rf "$tmp" "$staged"' EXIT
chmod u=rwx,go=rx "$staged"
cp -R "$tmp/repo/$path/." "$staged/"
echo "$sha" >"$staged/UPSTREAM_REF"
rm -rf "$dest"
mv "$staged" "$dest"

echo "synced $path/ from $repo at $sha into $dest"
