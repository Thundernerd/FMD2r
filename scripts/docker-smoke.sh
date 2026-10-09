#!/bin/sh
# Smoke-tests the FMD2r container image: the server answers, the SPA is embedded, the bundled Lua
# modules load, the tools upstream scripts call are on PATH, and there is no libfmdxpath.so (the
# image uses the native XPath backend only). CI's `docker` job and the release workflow run it.
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
  [ "$(docker inspect -f '{{.State.Running}}' "$name")" = true ] || fail "the container exited"
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

echo "native XPath backend only: no libfmdxpath.so, and fmd2r doesn't link it"
deps=$(docker exec "$name" ldd /usr/local/bin/fmd2r) || fail "ldd fmd2r"
case $deps in *libfmdxpath*) fail "fmd2r links libfmdxpath.so" ;; esac
lib=$(docker exec "$name" find / -xdev -name 'libfmdxpath*' 2>/dev/null || true)
[ -z "$lib" ] || fail "the image contains $lib"

echo "tools for upstream scripts"
docker exec "$name" magick -version >/dev/null || fail "magick -version"
docker exec "$name" python3 --version || fail "python3 --version"
docker exec "$name" node --version || fail "node --version"

# `module init` arrives with T15; until then the subcommand is a stub and this step is skipped.
# Once it is real, the step runs and fails loudly on any error (including a renamed flag).
if docker exec "$name" fmd2r module init 2>&1 | grep -q 'not implemented yet (T15)'; then
  echo "::warning::fmd2r module init is not implemented yet (T15); skipped"
else
  echo "fmd2r module init loads the bundled modules"
  docker exec "$name" fmd2r module init --lua-dir /data/lua >/dev/null || fail "fmd2r module init"
fi

echo "OK"
