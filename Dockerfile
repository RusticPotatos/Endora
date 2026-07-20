# syntax=docker/dockerfile:1
# Endora node — container image.
#
# Multi-stage: build the release binary, then ship it on a slim runtime.
# rusqlite is vendored (bundled SQLite), so the runtime needs no SQLite library.

# --- build stage -----------------------------------------------------------
# Pinned to the workspace MSRV; the builder image includes a C toolchain, which
# the bundled SQLite build requires.
FROM rust:1.87-bookworm AS builder
WORKDIR /app
COPY . .
# Cache the cargo registry/git and the target dir across builds (BuildKit cache
# mounts, kept on the build host). Only crates that actually changed recompile,
# so a normal source change is seconds instead of a full-workspace rebuild. The
# binary is copied OUT of the cache-mounted target within the same step, since a
# cache mount is not part of the image layer.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release -p endora-node \
    && cp target/release/endora-node /endora-node

# --- runtime stage ---------------------------------------------------------
FROM debian:bookworm-slim
# ca-certificates lets an optional HTTPS model endpoint be reached; SQLite is
# statically linked, so nothing else is needed.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /endora-node /usr/local/bin/endora-node

# Listen on all interfaces (so the mapped port is reachable) and keep the
# local-first database on a volume so it survives container restarts.
ENV ENDORA_ADDR=0.0.0.0:8787
ENV ENDORA_DB=/data/endora.db
VOLUME /data
EXPOSE 8787

ENTRYPOINT ["endora-node"]
