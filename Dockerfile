# FMD2r container image: the fmd2r binary (with the web UI embedded) and the tools upstream Lua
# modules shell out to (python3, node, ImageMagick's `magick`).
#
#   docker build -t fmd2r .
#   docker run -p 8080:8080 -v fmd2r-data:/data fmd2r
#
# linux/amd64 and linux/arm64 (`docker buildx build --platform linux/arm64 .`). fmd2r uses the
# native XPath backend only, so the image has no libfmdxpath.so (the fpc backend's x86-only
# shim). The web UI and fmd2r build on the build platform; fmd2r cross-compiles for the target,
# so only the runtime stage runs under emulation.

# --- Web UI (SvelteKit, static) -------------------------------------------------------------
FROM --platform=$BUILDPLATFORM node:24-trixie-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY openapi.json /src/openapi.json
COPY web/ ./
RUN npm run build

# --- fmd2r ---------------------------------------------------------------------------------
FROM --platform=$BUILDPLATFORM rust:1.97.1-slim-trixie AS rust
ARG BUILDARCH
ARG TARGETARCH
# The C dependencies (Lua, SQLite, ring, zstd) need a C cross compiler when the target differs.
RUN case "$TARGETARCH" in \
      amd64) arch=x86_64 pkg=x86-64 ;; \
      arm64) arch=aarch64 pkg=aarch64 ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && echo "$arch-unknown-linux-gnu" > /rust-target \
 && if [ "$TARGETARCH" != "$BUILDARCH" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends "gcc-$pkg-linux-gnu" "libc6-dev-$TARGETARCH-cross" \
      && rm -rf /var/lib/apt/lists/*; \
    fi
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar
# The commit shown by GET /api/about (the build context has no .git).
ARG FMD2R_GIT_REVISION=
ENV FMD2R_GIT_REVISION=${FMD2R_GIT_REVISION}
WORKDIR /src
COPY . .
COPY --from=web /src/web/build web/build
RUN target=$(cat /rust-target) \
 && rustup target add "$target" \
 && cargo build --release --locked -p fmd2r --target "$target" \
 && cp "target/$target/release/fmd2r" /usr/local/bin/fmd2r

# --- Runtime -------------------------------------------------------------------------------
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl imagemagick nodejs python3 tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1000 --user-group --home-dir /data --shell /usr/sbin/nologin fmd2r \
 && mkdir -p /data \
 && chown fmd2r:fmd2r /data

COPY --from=rust /usr/local/bin/fmd2r /usr/local/bin/fmd2r
# Seeds /data/lua on first start (docker/entrypoint.sh).
COPY fixtures/lua /opt/fmd2r/lua
COPY docker/entrypoint.sh /usr/local/bin/fmd2r-entrypoint

# Everything FMD2r writes lives in /data: app.db, lists.db, lua/, the cover cache, and
# downloads/ (the default save-to directory is relative, so it lands here).
ENV FMD2R_BIND=0.0.0.0:8080 \
    FMD2R_DATA_DIR=/data
# Optional, read by `fmd2r serve`:
#   FMD2R_PASSWORD          password/token the API requires (unset: no auth)
#   FMD2R_FLARESOLVERR_URL  e.g. http://flaresolverr:8191 for Cloudflare-protected sites
USER fmd2r
WORKDIR /data
VOLUME ["/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS "http://127.0.0.1:${FMD2R_BIND##*:}/api/health" >/dev/null || exit 1
ENTRYPOINT ["tini", "--", "fmd2r-entrypoint"]
CMD ["serve"]
