#!/usr/bin/env bash
# Rebuilds the XPath differential corpus (fixtures/xpath-corpus) from the smoke list's replay,
# then evaluates it on both XPath backends (needs fpc). See fixtures/README.md.
#
#   scripts/xpath-corpus.sh
#
# Run it after re-recording smoke entries, and commit the corpus with them.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q -p fmd2r -p fmd-smoke --features fmd2r/xpath-fpc
# fmd2r links libfmdxpath.so from the build directory, without an rpath.
LD_LIBRARY_PATH="$(realpath "$(dirname "$(ls -t target/debug/build/xpath-fpc-*/out/libfmdxpath.so | head -n1)")")"
export LD_LIBRARY_PATH
rm -rf fixtures/xpath-corpus
results="$(mktemp)"
target/debug/fmd-smoke run --xpath-corpus fixtures/xpath-corpus --out "$results"
# A step that fails to replay leaves its evaluations out of the corpus.
if grep -q '"passed": false' "$results"; then
  echo "error: some smoke entries failed to replay, see $results" >&2
  exit 1
fi
echo "$(wc -l < fixtures/xpath-corpus/entries.jsonl) corpus entries"
exec target/debug/fmd2r xpath diff --corpus fixtures/xpath-corpus --out /dev/null
