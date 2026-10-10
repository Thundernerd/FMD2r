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

## List metadata from MangaBaka

Settings → "MangaBaka database" downloads a local copy of
[MangaBaka](https://mangabaka.org)'s database (about 390 MB, refreshed every
`metadata.mangabaka.refresh_days` days). List titles are matched against it offline, which gives
Discover its format and publication facets and the series page a description when the website
has none. Nothing is downloaded until you ask, and no title leaves the server.

Discover's cards show the matched series' cover thumbnails from MangaBaka's CDN. A title without
a match gets its cover from its website's info page instead, looked up once per
`covers.revalidate_after_hours` and a few at a time per website. "Load manga covers" off shows
placeholders only.

MangaBaka's data is under its [data licence](https://mangabaka.org/about/data-license)
(CC BY-NC-SA 4.0); fields it takes from AniList, MyAnimeList, MangaUpdates and other providers
keep those providers' terms. FMD2r ships none of it: each install downloads its own.

## Custom stylesheet

For changes the settings don't offer, put a `custom.css` in the data folder (`--data-dir`;
Settings → Appearance shows where that is). The web UI loads it after its own styles, so a rule there
wins at equal specificity. The server reads it on every request: edit it and reload the page, no
restart needed. It is served without a login, so the login page uses it too. Files over 1 MiB are
refused.

```css
:root { --fs-md: 16px; --accent: #8a3ffc; }
:root[data-theme='dark'] { --bg: #000; }
```

Override the **design tokens** in `web/src/lib/styles/tokens.css`: they are the stable API. Class
names aren't, and may change between releases. The ones worth overriding:

- Colours: `--bg`, `--surface`, `--surface-2`, `--line`, `--fg`, `--muted`, `--accent`,
  `--accent-fg`, `--accent-soft`, and the status colours `--ok`, `--warn`, `--bad`, `--idle` with
  their `-soft` backgrounds. A colour set on `:root` applies in light and dark mode alike; give
  each mode its own with `light-dark(<light>, <dark>)`, e.g. `--bg: light-dark(#fff, #000);`.
  An `--accent` set here replaces whichever accent colour Settings → Appearance picks.
- Light or dark: Settings → Appearance sets `data-theme='light'` or `'dark'` on `<html>`, and
  none for "Same as the device". So the `:root[data-theme='dark']` rule above applies when Dark
  is picked; to match the device's dark mode too, add
  `@media (prefers-color-scheme: dark) { :root:not([data-theme='light']) { --bg: #000; } }`.
- Fonts and sizes: `--f-display`, `--f-body`, `--f-mono`, and `--fs-xs` to `--fs-hero`
  (`--fs-md` is body text). Each size is a multiple of `--text-scale`, the Text size setting: a
  fixed `16px` ignores that setting, while `calc(16px * var(--text-scale))` keeps it working.
- Spacing and corners: `--sp-1` to `--sp-6`, `--r`, `--r-lg`, `--r-xl`.

An `@import` of a web font or another stylesheet by URL works. In Docker the data folder is the
`/data` volume, so `custom.css` sits next to the databases there.

In `npm run dev`, Vite injects the app's styles after `custom.css`, so an override there may need
`!important`; a production build (and the embedded web UI) doesn't.

## Run with Docker

The image (`ghcr.io/thundernerd/fmd2r`, built from `Dockerfile`) holds the `fmd2r` binary with the
web UI embedded and the tools upstream modules shell out to: `python3`, `node` and ImageMagick's
`magick`. `compose.yaml` runs it with a FlareSolverr sidecar:

```sh
cp .env.example .env    # optional: port, password, library directory, image tag
mkdir -p manga          # downloads land here; it must be writable by uid 1000
docker compose up -d    # in a source checkout, `--build` builds the image locally
```

Then open <http://localhost:8080>. While the repository is private its GHCR package is too: run
`docker login ghcr.io` first, or build locally with `--build`.

On an amd64 host a local build works with or without BuildKit (the classic builder, used when the
buildx plugin is missing, builds a linux/amd64 image); other hosts need buildx. The build context
has no `.git`, so `FMD2R_GIT_REVISION` supplies the commit that `GET /api/about` and
the System page report:

```sh
FMD2R_GIT_REVISION=$(git rev-parse --short=12 HEAD) docker compose up -d --build
```

- **Data:** everything lives in `/data` (the `fmd2r-data` volume): `app.db`, `lists.db`, `lua/`, the
  cover cache, and `downloads/`, the default save-to directory, which compose maps to `MANGA_DIR`.
  (`downloads` is relative to the working directory, `/data`, not to `FMD2R_DATA_DIR`.)
  The container runs as the non-root user `fmd2r` (uid 1000), so bind-mounted directories must be
  writable by that uid.
- **Several download folders:** Settings → Save to holds named destinations (one of them the
  default); the series page, the library and each website (Settings → Website modules) pick one.
  To keep, say, manga and manhwa on different disks, mount each host folder into the container
  and add a destination for its container path:

  ```yaml
  services:
    fmd2r:
      volumes:
        - fmd2r-data:/data
        - ${MANGA_DIR:-./manga}:/data/downloads   # the default destination, "Downloads"
        - /mnt/disk1/manga:/data/manga             # destination "Manga" → /data/manga
        - /mnt/disk2/manhwa:/data/manhwa           # destination "Manhwa" → /data/manhwa
  ```

  A destination whose folder is missing (a disk not mounted yet) gets a warning in Settings, not
  an error.
- **Lua modules:** on first start, when `/data/lua` has no modules, it is seeded from the upstream
  snapshot baked into the image (`fixtures/lua`); existing files are never overwritten.
- **Configuration:** `serve` reads `FMD2R_BIND` (default `0.0.0.0:8080` in the image),
  `FMD2R_DATA_DIR` (`/data`), `FMD2R_PASSWORD` and `FMD2R_FLARESOLVERR_URL` (compose sets
  `http://flaresolverr:8191`). Each overrides its setting (Settings → Server: Listen address and
  Password; Connections: FlareSolverr URL), and the Settings page says so. Without
  `FMD2R_PASSWORD` the password comes from the settings, and with neither the API is open: the
  server then logs a warning and the UI shows a banner when it listens beyond loopback.
- **Health:** the image's `HEALTHCHECK` polls `GET /api/health`.
- **Platforms:** linux/amd64 and linux/arm64. `fmd2r` uses the native XPath backend only, so the
  image has no `libfmdxpath.so` (the `fpc` backend's shim, whose float-environment code is x86
  assembly). The Dockerfile cross-compiles `fmd2r` for the target platform:
  `docker buildx build --platform linux/arm64 .` needs QEMU only for the runtime stage.

### Releases

Pushing a version tag (`v1.2.3`, `v1.2.3-rc.1`) runs `.github/workflows/release.yml`: it builds the
image for linux/amd64 and linux/arm64 in parallel, each natively on its own runner (arm64 on
GitHub's `ubuntu-24.04-arm`), smoke-tests each (`scripts/docker-smoke.sh`) and pushes it to GHCR
by digest, untagged. Once both platforms pass, a final job combines the two digests into one
multi-platform image tagged `<version>`, `<major>.<minor>` and `latest` (pre-release tags skip
`latest`), so nothing is tagged unless both platforms passed. The workflow then creates a GitHub
release with `fmd2r-<tag>-x86_64-linux-gnu.tar.gz` and `fmd2r-<tag>-aarch64-linux-gnu.tar.gz` (the
binary, built on Ubuntu 22.04 so it needs glibc 2.35 or newer; each smoke-tested with
`scripts/tarball-smoke.sh`) and their `SHA256SUMS`.

PRs that change `release.yml` or the scripts it runs, and manual runs, are dry runs: they build and
smoke-test both platforms and the tarballs but push and release nothing.

The tag sets the version: before building, `scripts/set-version.sh` writes it into `Cargo.toml` and
`Cargo.lock`, so `fmd2r --version` and `GET /api/about` report it without a version bump on main,
and the smoke tests check that they do.

## CI

The workflows in `.github/workflows` are set up for fast feedback on PRs: a new push to a PR
cancels that PR's runs still in progress (runs on `main` always finish), and the slow jobs run
only when their inputs change.

| Workflow | Runs on | What it checks |
| --- | --- | --- |
| `ci.yml` | every PR and push to `main` | fmt, clippy, `cargo test` (native XPath backend, smoke replay, module corpus), and the web UI |
| `xpath-fpc.yml` | PRs and pushes to `main` that touch the fpc shim, `fmd-xpath`, `fmd-lua`'s XPath binding, `fmd2r xpath`, `fixtures/xpath-corpus` or the manifests; nightly; manual | the `fpc` backend's parity with the native one, and uploads `libfmdxpath.so` |
| `docker.yml` | PRs and pushes to `main` that touch what the image is built from; manual | builds the image for amd64 and arm64 and runs `scripts/docker-smoke.sh` on each (arm64 under QEMU) |
| `smoke-nightly.yml` | nightly; manual | the smoke list live and replayed, and a native/fpc diff on the day's pages |
| `release.yml` | version tags; dry runs on PRs that touch it or its scripts; manual | see [Releases](#releases) |

To run a path-filtered workflow on a branch whose changes it skipped, start it by hand from the
Actions tab ("Run workflow") or with `gh workflow run xpath-fpc.yml --ref <branch>` (likewise
`docker.yml`).
