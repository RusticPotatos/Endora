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
RUN cargo build --release -p endora-node

# --- runtime stage ---------------------------------------------------------
FROM debian:bookworm-slim
# ca-certificates lets an optional HTTPS model endpoint be reached; SQLite is
# statically linked, so nothing else is needed.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/endora-node /usr/local/bin/endora-node

# Listen on all interfaces (so the mapped port is reachable) and keep the
# local-first database on a volume so it survives container restarts.
ENV ENDORA_ADDR=0.0.0.0:8787
ENV ENDORA_DB=/data/endora.db
VOLUME /data
EXPOSE 8787

ENTRYPOINT ["endora-node"]
