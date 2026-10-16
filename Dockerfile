# syntax=docker/dockerfile:1
# Endora node — container image.
#
# Multi-stage: build the release binary, then ship it on a slim runtime.
# rusqlite is vendored (bundled SQLite), so the runtime needs no SQLite library.

# --- build stage -----------------------------------------------------------
# Pinned to the workspace MSRV; the builder image includes a C toolchain, which
# the bundled SQLite build requires.
#
# The builder runs on the machine that BUILDS ($BUILDPLATFORM) and compiles for the one
# that RUNS ($TARGETPLATFORM): on a laptop deploying to a NAS of another architecture it
# cross-compiles natively instead of emulating, so the NAS never compiles at all. A build
# for the same architecture is an ordinary build.
FROM --platform=$BUILDPLATFORM rust:1.87-bookworm AS builder
ARG TARGETARCH
# rust-toolchain.toml asks for "stable", which made every build download a whole toolchain
# (minutes, and a failed build whenever that download timed out) and compile the release
# with a different Rust from the one the tests ran on. The image's own toolchain is used.
ENV RUSTUP_TOOLCHAIN=1.87.0
# The target's Rust standard library and C cross-compiler, only when crossing. A layer of
# its own, so it is fetched once per builder rather than per build.
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-gnu; gcc=gcc-x86-64-linux-gnu ;; \
      arm64) triple=aarch64-unknown-linux-gnu; gcc=gcc-aarch64-linux-gnu ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    if [ "$triple" != "$(rustc -vV | sed -n 's/^host: //p')" ]; then \
      rustup target add "$triple"; \
      apt-get update; \
      apt-get install -y --no-install-recommends "$gcc" libc6-dev-"$TARGETARCH"-cross; \
      rm -rf /var/lib/apt/lists/*; \
    fi; \
    echo "$triple" > /target-triple
WORKDIR /app
COPY . .
# Cache the cargo registry/git and the target dir across builds (BuildKit cache
# mounts, kept on the build host). Only crates that actually changed recompile,
# so a normal source change is seconds instead of a full-workspace rebuild. The
# binary is copied OUT of the cache-mounted target within the same step, since a
# cache mount is not part of the image layer.
#
# `touch` the sources first: BuildKit's COPY normalizes file mtimes to a constant,
# so a changed source can end up OLDER than the cached target/ artifacts and
# cargo's mtime check skips recompiling it — serving a stale binary. Bumping the
# mtimes forces cargo to re-fingerprint; it still recompiles only crates whose
# content actually changed, so incremental builds stay fast.
#
# Crossing, cargo and the C builds it drives (SQLite, ring) are pointed at the target's
# compiler; for a native build the variables name a compiler that is unused.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target,sharing=locked \
    set -eu; \
    triple="$(cat /target-triple)"; \
    cross="$(echo "$triple" | sed 's/-unknown-/-/')"; \
    env_triple="$(echo "$triple" | tr 'a-z-' 'A-Z_')"; \
    export "CARGO_TARGET_${env_triple}_LINKER=${cross}-gcc"; \
    export "CC_$(echo "$triple" | tr '-' '_')=${cross}-gcc"; \
    export "AR_$(echo "$triple" | tr '-' '_')=${cross}-ar"; \
    if [ "$triple" = "$(rustc -vV | sed -n 's/^host: //p')" ]; then \
      unset "CARGO_TARGET_${env_triple}_LINKER" "CC_$(echo "$triple" | tr '-' '_')" "AR_$(echo "$triple" | tr '-' '_')"; \
    fi; \
    find . \( -name '*.rs' -o -name '*.toml' -o -name '*.lock' \) -exec touch {} + \
    && cargo build --release -p endora-node --target "$triple" \
    && cp "target/$triple/release/endora-node" /endora-node

# --- runtime stage ---------------------------------------------------------
FROM debian:bookworm-slim
# ca-certificates lets an optional HTTPS model endpoint be reached; SQLite is
# statically linked. nodejs+npm (for `npx …` servers) and python3 (for python
# servers) are the two runtimes the vast majority of stdio MCP servers need
# (ADR 0021) — without them a registered server's command isn't found and it stays
# unconnected. Node's `npx` fetches a package on first run, so a freshly registered
# npx server may need one retry while it downloads; a globally pre-installed server
# (or one already cached) connects immediately.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates nodejs npm python3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /endora-node /usr/local/bin/endora-node
COPY scripts/homelab-docker-proxy.py /usr/local/lib/endora/homelab-docker-proxy.py

# Listen on all interfaces (so the mapped port is reachable) and keep the
# local-first database on a volume so it survives container restarts.
ENV ENDORA_ADDR=0.0.0.0:8787
ENV ENDORA_DB=/data/endora.db
# Build identifier (the deploy's git short SHA), surfaced in /health and the
# console header so a refresh can tell one build from the next.
ARG ENDORA_BUILD=dev
ENV ENDORA_BUILD=$ENDORA_BUILD
VOLUME /data
EXPOSE 8787

ENTRYPOINT ["endora-node"]
