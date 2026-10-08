# FMD2r

Rust port of FMD2 with a web UI. See `docs/plan.md` for the plan and `docs/tickets/` for the work.

The upstream Lua modules used by the tests live in `fixtures/lua`; see `fixtures/README.md`.

## Cloudflare-protected sites

After every module request FMD2r runs upstream's `lua/websitebypass/checkantibot.lua`; on a
Cloudflare or DDoS-Guard challenge it runs `websitebypass.lua` and keeps the cookies and user
agent it obtains in the module's settings. Cloudflare challenges need
[FlareSolverr](https://github.com/FlareSolverr/FlareSolverr):

```sh
docker compose -f compose.dev.yaml up -d
cargo run -p fmd2r -- serve --flaresolverr-url http://localhost:8191
```

## Run with Docker

The image (`ghcr.io/thundernerd/fmd2r`, built from `Dockerfile`) holds the `fmd2r` binary with the
web UI embedded, `libfmdxpath.so`, and the tools upstream modules shell out to: `python3`, `node`
and ImageMagick's `magick`. `compose.yaml` runs it with a FlareSolverr sidecar:

```sh
cp .env.example .env    # optional: port, password, library directory, image tag
mkdir -p manga          # downloads land here; it must be writable by uid 1000
docker compose up -d    # in a source checkout, `--build` builds the image locally
```

Then open <http://localhost:8080>. While the repository is private its GHCR package is too: run
`docker login ghcr.io` first, or build locally with `--build`.

- **Data:** everything lives in `/data` (the `fmd2r-data` volume): `app.db`, `lists.db`, `lua/`, the
  cover cache, and `downloads/`, the default save-to directory, which compose maps to `MANGA_DIR`.
  (`downloads` is relative to the working directory, `/data`, not to `FMD2R_DATA_DIR`.)
  The container runs as the non-root user `fmd2r` (uid 1000), so bind-mounted directories must be
  writable by that uid.
- **Lua modules:** on first start, when `/data/lua` has no modules, it is seeded from the upstream
  snapshot baked into the image (`fixtures/lua`); existing files are never overwritten.
- **Configuration:** `serve` reads `FMD2R_BIND` (default `0.0.0.0:8080` in the image),
  `FMD2R_DATA_DIR` (`/data`), `FMD2R_PASSWORD` (unset: no auth) and `FMD2R_FLARESOLVERR_URL`
  (compose sets `http://flaresolverr:8191`).
- **Health:** the image's `HEALTHCHECK` polls `GET /api/health`.
- **Platforms:** linux/amd64 only. `libfmdxpath.so`'s float-environment code
  (`crates/xpath-fpc/pascal/fxfpu.pas`) is x86 assembly, so FMD2's XPath engine doesn't build for
  arm64.

### Releases

Pushing a version tag (`v1.2.3`, `v1.2.3-rc.1`) runs `.github/workflows/release.yml`: it builds and smoke-tests the image
(`scripts/docker-smoke.sh`), pushes it to GHCR as `<version>`, `<major>.<minor>` and `latest`
(pre-release tags skip `latest`), and creates a GitHub release with
`fmd2r-<tag>-x86_64-linux-gnu.tar.gz` (the binary and `libfmdxpath.so`, built on Ubuntu 22.04 so
it needs glibc 2.35 or newer) and its `SHA256SUMS`.
