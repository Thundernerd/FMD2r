#!/usr/bin/env bash
# Writes fmd-server's OpenAPI document (default: openapi.json at the repo root), which `web/`
# generates its API client from (`npm run gen:api`).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/openapi.json}"
cargo run --quiet --manifest-path "$root/Cargo.toml" -p fmd2r -- openapi --out "$out"
echo "wrote $out"
