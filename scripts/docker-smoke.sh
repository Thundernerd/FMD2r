#!/bin/sh
# Smoke-tests the FMD2r container image: the server answers, the SPA is embedded, the bundled Lua
# modules load, and the tools upstream scripts call are on PATH. CI's `docker` job runs it.
#
# Usage: scripts/docker-smoke.sh [IMAGE]   (default: fmd2r:smoke)
set -eu

image=${1:-fmd2r:smoke}
name=fmd2r-smoke-$$

fail() {
  echo "FAIL: $*" >&2
  docker logs "$name" >&2 || true
  exit 1
}

cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$name" -p 127.0.0.1::8080 "$image" >/dev/null
port=$(docker port "$name" 8080/tcp | head -n1 | sed 's/.*://')
base=http://127.0.0.1:$port

echo "waiting for $base/api/health"
i=0
until [ "$(curl -s -o /dev/null -w '%{http_code}' "$base/api/health")" = 200 ]; do
  i=$((i + 1))
  [ "$i" -lt 60 ] || fail "/api/health never answered 200"
  sleep 1
done

echo "GET / serves the SPA"
curl -sf "$base/" | grep -q '__sveltekit' || fail "GET / did not return the SPA"

echo "runs as non-root"
uid=$(docker exec "$name" id -u)
[ "$uid" != 0 ] || fail "the container runs as root"

echo "first start seeded /data/lua"
docker exec "$name" sh -c 'ls /data/lua/modules/*.lua >/dev/null' || fail "/data/lua/modules is empty"

echo "tools for upstream scripts"
docker exec "$name" magick -version >/dev/null || fail "magick -version"
docker exec "$name" python3 --version || fail "python3 --version"
docker exec "$name" node --version || fail "node --version"

# `module init` arrives with T15; until then the binary has no --lua-dir and this step is skipped.
if docker exec "$name" fmd2r module init --help 2>/dev/null | grep -q -- '--lua-dir'; then
  echo "fmd2r module init loads the bundled modules"
  docker exec "$name" fmd2r module init --lua-dir /data/lua >/dev/null || fail "fmd2r module init"
else
  echo "::warning::fmd2r module init has no --lua-dir yet (T15); skipped"
fi

echo "OK"
