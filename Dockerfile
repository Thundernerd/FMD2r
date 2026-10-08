# FMD2r container image: the fmd2r binary (with the web UI embedded), libfmdxpath.so, and the
# tools upstream Lua modules shell out to (python3, node, ImageMagick's `magick`).
#
#   docker build -t fmd2r .
#   docker run -p 8080:8080 -v fmd2r-data:/data fmd2r
#
# amd64 only: libfmdxpath.so's float-environment code (crates/xpath-fpc/pascal/fxfpu.pas) is
# x86 assembly. See README.md, "Run with Docker".

# --- Web UI (SvelteKit, static) -------------------------------------------------------------
FROM node:24-trixie-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY openapi.json /src/openapi.json
COPY web/ ./
RUN npm run build

# --- libfmdxpath.so (FMD2's XPath engine, Free Pascal) ---------------------------------------
FROM debian:trixie-slim AS xpath
RUN apt-get update \
 && apt-get install -y --no-install-recommends fpc binutils ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY crates/xpath-fpc/build.sh crates/xpath-fpc/fmdxpath.h crates/xpath-fpc/
COPY crates/xpath-fpc/pascal crates/xpath-fpc/pascal
RUN crates/xpath-fpc/build.sh /out

# --- fmd2r ---------------------------------------------------------------------------------
FROM rust:1.97.1-slim-trixie AS rust
# The commit shown by GET /api/about (the build context has no .git).
ARG FMD2R_GIT_REVISION=
ENV FMD2R_GIT_REVISION=${FMD2R_GIT_REVISION}
WORKDIR /src
COPY . .
COPY --from=web /src/web/build web/build
# Crates built with the fpc XPath backend link this prebuilt library instead of running fpc.
COPY --from=xpath /out/libfmdxpath.so /usr/local/lib/
ENV FMDXPATH_LIB_DIR=/usr/local/lib
RUN cargo build --release --locked -p fmd2r \
 && cp target/release/fmd2r /usr/local/bin/fmd2r

# --- Runtime -------------------------------------------------------------------------------
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl imagemagick nodejs python3 tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1000 --user-group --home-dir /data --shell /usr/sbin/nologin fmd2r \
 && mkdir -p /data \
 && chown fmd2r:fmd2r /data

COPY --from=xpath /out/libfmdxpath.so /usr/local/lib/
RUN ldconfig
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
