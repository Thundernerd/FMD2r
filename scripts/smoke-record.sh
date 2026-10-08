#!/usr/bin/env bash
# Re-records smoke list entries (fixtures/smoke/list.toml) from the live sites: their HTTP
# fixtures and the info/pages snapshots the CI replay compares against. See fixtures/README.md.
#
#   scripts/smoke-record.sh mangadex            # one entry
#   scripts/smoke-record.sh mangadex mangaplus  # several
#
# Review the snapshot diff (`git diff fixtures/smoke`) before committing.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q -p fmd2r -p fmd-smoke
exec target/debug/fmd-smoke record "$@"
