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
# Cargo judges our own crates stale by mtime, and here mtimes lie: the target cache is
# shared by every checkout that deploys from this machine, so a changed source can be
# OLDER than an artifact another checkout cached, and cargo would ship that stale
# binary. scripts/fresh-sources.sh decides by content instead: it keeps the hash of
# every file the cached build was made from, gives unchanged files an old mtime and
# changed ones the present, so exactly the crates that changed recompile. (Touching
# every source, as this once did, rebuilt the whole workspace on every deploy.)
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
    bins="endora-node endora-hq endora-worker endora-runner endora-sound-hub"; \
    built() { for b in $bins; do echo "target/$triple/release/$b"; done; }; \
    sh scripts/fresh-sources.sh before "target/$triple" $(built) \
    && cargo build --release -p endora-node -p endora-hq -p endora-worker -p endora-runner -p endora-sound-hub --target "$triple" \
    && sh scripts/fresh-sources.sh after "target/$triple" $(built) \
    && cp "target/$triple/release/endora-node" /endora-node \
    && cp "target/$triple/release/endora-hq" /endora-hq \
    && cp "target/$triple/release/endora-worker" /endora-worker \
    && cp "target/$triple/release/endora-runner" /endora-runner \
    && cp "target/$triple/release/endora-sound-hub" /endora-sound-hub

# endora-sound's AirPlay senders (ADR 0104): stock OwnTone, built from its release, plus the
# airptpd clock it ships the source of but no image builds, which four instances must share.
# Built with `--target senders`; nothing of OwnTone's (GPL) is linked into anything of ours,
# it runs beside the hub and is driven over its JSON API.
FROM alpine:3.23 AS owntone-build
ARG OWNTONE_VERSION=29.3
RUN apk add -U -q --no-cache --no-progress \
      alsa-lib-dev autoconf automake avahi-dev bison confuse-dev curl-dev ffmpeg-dev flex \
      g++ gawk gcc gettext-dev git gnutls-dev gperf json-c-dev libevent-dev libgcrypt-dev \
      libplist-dev libsodium-dev libtool libunistring-dev libwebsockets-dev libxml2-dev make \
      protobuf-c-dev sqlite-dev
WORKDIR /tmp/source
RUN git clone --depth 1 -b "$OWNTONE_VERSION" https://github.com/owntone/owntone-server.git . \
    && cp -r src/libairptp /tmp/airptp \
    && autoreconf -fi -I /usr/share/gettext/m4 \
    && ./configure --disable-install_systemd --disable-install_user --enable-silent-rules \
         --prefix=/usr --sysconfdir=/etc/owntone --localstatedir=/var \
    && make DESTDIR=/tmp/build install \
    && cd /tmp/airptp \
    && autoreconf -fi && ./configure --enable-daemon --prefix=/usr && make \
    && install -D daemon/airptpd /tmp/build/usr/sbin/airptpd \
    && rm -rf /tmp/build/var /tmp/build/etc
# The hub's AirPlay receivers ("Endora · iPhone" and so on): shairport-sync, built for classic
# AirPlay, which needs no PTP clock of its own and so shares the host with airptpd (an AirPlay 2
# build's nqptp would want the same ports; the owner chose classic, 2026-10-17). Its own layer,
# so OwnTone's build above stays cached.
ARG SHAIRPORT_VERSION=5.6
RUN apk add -U -q --no-cache --no-progress popt-dev libconfig-dev openssl-dev \
    && git clone --depth 1 -b "$SHAIRPORT_VERSION" https://github.com/mikebrady/shairport-sync.git /tmp/shairport \
    && cd /tmp/shairport \
    && autoreconf -fi \
    && ./configure --with-pipe --with-avahi --with-ssl=openssl --prefix=/usr --sysconfdir=/etc \
    && make \
    && install -D shairport-sync /tmp/build/usr/bin/shairport-sync

FROM alpine:3.23 AS senders
RUN apk add -U -q --no-cache --no-progress \
      alsa-lib avahi confuse curl dbus ffmpeg gnutls json-c libevent libgcrypt libplist \
      libsodium libunistring libuuid libwebsockets libxml2 protobuf-c sqlite sqlite-libs \
      popt libconfig openssl su-exec \
    && rm -f /etc/avahi/services/* \
    && adduser -S -H -D owntone
COPY --from=owntone-build /tmp/build/ /
COPY --chmod=755 scripts/sound-senders.sh /usr/local/bin/sound-senders
VOLUME /data
ENTRYPOINT ["/usr/local/bin/sound-senders"]

# endora-sound (ADR 0104): the sound hub, in an image of its own, because it needs ffmpeg and
# the node does not. Built with `--target sound`. It sits before the node's stage on purpose:
# a plain build still makes the node, the last stage.
FROM debian:bookworm-slim AS sound
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /endora-sound-hub /usr/local/bin/endora-sound-hub
ENV SOUND_ADDR=0.0.0.0:8791
ENV SOUND_STATE=/data/sound.json
ENV SOUND_PIPES=/pipes
ENV SOUND_TOKEN_FILE=/token/token
VOLUME /data
EXPOSE 8791
ENTRYPOINT ["endora-sound-hub"]

# The workbench (ring 1): a team member's disposable sandbox on the Pi, one container a turn,
# where the worker gives the model a real shell. Built with `--target workbench` and tagged
# endora-workbench:latest:
#   docker buildx build --platform linux/arm64 --target workbench -t endora-workbench:latest --load .
# It is not the node's image: the node keeps the last stage. Run with a read-only root, /work as
# a tmpfs owned by the workbench user, and HTTPS_PROXY (and https_proxy, which curl and git read
# for plain HTTP) naming Endora's egress proxy, its only way out. The proxy does not decrypt TLS,
# so the system's own certificates are all the sandbox trusts.
FROM debian:bookworm-slim AS workbench
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash ca-certificates curl dnsutils git iputils-ping jq less procps python3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --home-dir /work --no-create-home --shell /bin/bash workbench
COPY --from=builder /endora-worker /usr/local/bin/endora-worker
ENV HOME=/work
ENV ENDORA_WORKBENCH=1
WORKDIR /work
USER workbench
ENTRYPOINT ["endora-worker"]

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
        ca-certificates git nodejs npm python3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /endora-node /usr/local/bin/endora-node
# Endora HQ, the team service (ADR 0102): the same image, run as its own container.
COPY --from=builder /endora-hq /usr/local/bin/endora-hq
# The team's worker, which takes one turn in a fresh container, and the runner, which starts
# those containers (ADR 0102, phase 4). The same image again, so a worker runs nothing that
# was not built here.
COPY --from=builder /endora-worker /usr/local/bin/endora-worker
COPY --from=builder /endora-runner /usr/local/bin/endora-runner
COPY scripts/homelab-docker-proxy.py /usr/local/lib/endora/homelab-docker-proxy.py

# Listen on all interfaces (so the mapped port is reachable) and keep the
# local-first database on a volume so it survives container restarts.
ENV ENDORA_ADDR=0.0.0.0:8787
ENV ENDORA_DB=/data/endora.db
# What Endora knows about the person, as markdown pages kept in git (ADR 0101): git is the
# history, run with hooks and outside configuration off.
ENV ENDORA_BRAIN=/brain
# Build identifier (the deploy's git short SHA), surfaced in /health and the
# console header so a refresh can tell one build from the next.
ARG ENDORA_BUILD=dev
ENV ENDORA_BUILD=$ENDORA_BUILD
VOLUME /data
EXPOSE 8787

ENTRYPOINT ["endora-node"]
