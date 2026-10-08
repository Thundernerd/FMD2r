#!/bin/sh
# Seeds the data volume's lua/ from the snapshot baked into the image (fixtures/lua, the
# upstream tree the tests run against) when it has no modules yet, then runs fmd2r. Files
# already in lua/ are kept, so a tree the module updater (T29) synced is never overwritten.
set -eu

data=${FMD2R_DATA_DIR:-/data}
mkdir -p "$data/lua"
if ! ls "$data/lua/modules/"*.lua >/dev/null 2>&1; then
  echo "seeding $data/lua from the bundled snapshot ($(cat /opt/fmd2r/lua/UPSTREAM_REF 2>/dev/null || echo unknown))" >&2
  cp -R --update=none /opt/fmd2r/lua/. "$data/lua/"
fi

exec fmd2r "$@"
