# T55: Local Docker builds: work without BuildKit, report the git revision
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification (F2). Since T49, `Dockerfile:13` and `Dockerfile:22` use `FROM --platform=$BUILDPLATFORM`, and `BUILDARCH`/`TARGETARCH` (`Dockerfile:23-24`) have no defaults. On a host without the buildx plugin, `docker compose up --build` falls back to the classic builder, which doesn't set these, and stops at step 1:
```
failed to parse platform : "" is an invalid OS component of "" …: invalid argument
```
`--build-arg BUILDPLATFORM=…` doesn't help because there is no global `ARG BUILDPLATFORM` before the first `FROM`.

Also, a local build reports `git_revision: null` in `/api/about`: `.dockerignore` excludes `.git`, so `crates/fmd-server/build.rs` can't ask git, and `compose.yaml` passes no `FMD2R_GIT_REVISION` build arg (`Dockerfile:44`; only the CI workflows set it, `.github/workflows/docker.yml:61`, `release.yml`).

## Scope (in/out)
In:
- Global `ARG BUILDPLATFORM=linux/amd64` before the first `FROM`, and defaults for `BUILDARCH`/`TARGETARCH` (amd64), so the classic builder builds an amd64 image. BuildKit still overrides them. If that can't work, the README states that BuildKit/buildx is required.
- `compose.yaml` passes `FMD2R_GIT_REVISION` as a build arg from an environment variable (documented in `.env.example`), so a local build can report its revision.

Out: arm64 builds without buildx. The CI workflows.

## Seams under test
- `DOCKER_BUILDKIT=0 docker build .` succeeds (amd64 host), and `docker buildx build --platform linux/arm64 .` still cross-compiles (the existing Docker workflow).
- With `FMD2R_GIT_REVISION=$(git rev-parse --short=12 HEAD) docker compose build`, `/api/about` reports that revision.

## Acceptance criteria
- [ ] `docker compose up --build` works on a host without the buildx plugin.
- [ ] The Docker workflow on `main` stays green for amd64 and arm64.
- [ ] A local compose build can set `git_revision`; the README says how.

## FMD2 references
None (packaging only).
