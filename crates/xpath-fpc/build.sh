#!/bin/sh
# Builds libfmdxpath.so: fetches the pinned internettools revision and compiles pascal/fmdxpath.lpr with fpc.
#
# Usage: crates/xpath-fpc/build.sh [OUT_DIR]   (default: crates/xpath-fpc/build)
# Needs: fpc (3.2.2), curl, tar.
set -eu

# The internettools fork that FMD2's scripts/install_submodules.bat clones (benibela/internettools plus FLRE and PUCU).
INTERNETTOOLS_REPO=Slasar41/internettools
INTERNETTOOLS_REV=a547b0f7c69d2be3b9d7236cdccee1b10580495d

here=$(cd "$(dirname "$0")" && pwd)
out=${1:-$here/build}
mkdir -p "$out"
out=$(cd "$out" && pwd)

src=$out/vendor/internettools-$INTERNETTOOLS_REV
if [ ! -d "$src" ]; then
  mkdir -p "$out/vendor"
  curl -sSLf "https://codeload.github.com/$INTERNETTOOLS_REPO/tar.gz/$INTERNETTOOLS_REV" | tar -xz -C "$out/vendor"
fi

mkdir -p "$out/units"
# -MObjFPC -Scghi are Lazarus' default package options, which FMD2 builds internettools with.
fpc -MObjFPC -Scghi -O2 -gl -B -ve \
  -Fi"$src/data" -Fu"$src/data" -Fu"$src/internet" -Fu"$src/system" \
  -FU"$out/units" -o"$out/libfmdxpath.so" \
  "$here/pascal/fmdxpath.lpr"

# The header and the library must export the same functions.
declared=$(grep -oE '\bfx_[a-z_]+\(' "$here/fmdxpath.h" | tr -d '(' | sort -u)
exported=$(nm -D --defined-only "$out/libfmdxpath.so" | awk '{print $3}' | grep -E '^fx_' | sort -u)
if [ "$declared" != "$exported" ]; then
  printf 'fmdxpath.h and libfmdxpath.so disagree.\nDeclared:\n%s\nExported:\n%s\n' "$declared" "$exported" >&2
  exit 1
fi
