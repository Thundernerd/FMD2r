# T49: arm64 images now that the native XPath backend is the default
Deps: none

## Goal
Images and release tarballs are amd64-only because `libfmdxpath.so` is x86 assembly (PR #50). Since T35 the binary in the image is built with the native backend only, so the image ships `libfmdxpath.so` without using it. Drop it and build for arm64 too.

## Scope (in/out)
In:
- `Dockerfile`: remove the `xpath` (fpc) stage and the `libfmdxpath.so` copy; confirm `fmd2r` doesn't link it (`ldd`).
- `release.yml` and `docker.yml`: build `linux/amd64` and `linux/arm64` (QEMU or native arm runners); add an arm64 tarball. Update the "amd64 only" comments and README.
- Keep the `xpath-fpc` CI job as the parity guard.

Out: shipping the fpc backend in release artifacts.

## Seams under test
`scripts/docker-smoke.sh` against both architectures' images; `fmd2r --version` and `module init` in the arm64 tarball under QEMU.

## Acceptance criteria
- [ ] Multi-arch image pushed on a tag; smoke test passes on both.
- [ ] Image no longer contains `libfmdxpath.so`.

## FMD2 references
- None.
