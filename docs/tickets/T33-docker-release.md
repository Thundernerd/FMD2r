# T33: Docker image and release builds
Deps: T21, T30

## Goal
Ship FMD2r as one container image (binary + embedded SPA + `libfmdxpath.so` + python3, node and ImageMagick for upstream scripts) with a compose file including FlareSolverr, plus release binaries built in CI.

## Scope (in/out)
In:
- Multi-stage `Dockerfile`: build web (`node`), build `libfmdxpath.so` (`fpc`), build Rust (release, `--locked`); runtime on a slim Debian with `python3`, `nodejs`, `imagemagick` (`magick` on PATH), CA certs, tini; non-root user; `/data` volume (app.db, lists.db, lua/, covers cache, downloads default); `EXPOSE 8080`; `HEALTHCHECK` on `/api/health`; env vars for bind address, data dir, auth token, FlareSolverr URL.
- `compose.yaml`: `fmd2r` + `flaresolverr` (ghcr.io/flaresolverr/flaresolverr) with the bypass config pointing at it; volumes for data and manga library; example `.env`.
- First start bootstraps `lua/` via the module updater (T29) when empty (or bakes a snapshot into the image as fallback).
- GitHub Actions release workflow on tags: multi-arch image (amd64, arm64 if FPC cross-build is feasible; else amd64 only, documented) pushed to GHCR; Linux release tarball with binary + `.so`; checksums.
- README "Run with Docker" section.

Out: Windows/macOS packaging.

## Seams under test
- CI smoke job: build the image, `docker run` it, poll `/api/health` until 200, `GET /` returns the SPA, `fmd2r module init` inside the container loads the bundled/bootstrapped modules, `magick -version`, `python3 --version`, `node --version` succeed in the runtime image.
- `docker compose config` validates.

## Acceptance criteria
- [ ] `docker compose up` gives a working UI on port 8080 with FlareSolverr reachable from the app.
- [ ] Image runs as non-root; data persisted in the volume.
- [ ] Release workflow produces image + tarball on a tag.

## FMD2 references
- `lua/websitebypass/websitebypass_config.json` (FlareSolverr IP/port keys)
- `lua/utils/nodejs.lua:41`, `lua/websitebypass/cloudflare.lua:344` (node/python requirements)
- `baseunits/imagemagickmanager.pas:253-334` (`FindMagickBinary`)
