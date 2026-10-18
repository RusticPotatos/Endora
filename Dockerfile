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
# The hub's classic AirPlay door ("Endora Mixer"): shairport-sync built for classic AirPlay,
# which needs no PTP clock of its own and so shares the host with airptpd. Since ADR 0128 it is
# the fall-back: with SOUND_AIRPLAY2=1 the door is the AirPlay 2 one (the airplay2 stage below,
# on a LAN address of its own) and this build runs nothing. Its own layer, so OwnTone's build
# above stays cached.
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

# endora-sound's AirPlay 2 door (ADR 0128): shairport-sync built for AirPlay 2 and the nqptp
# clock it needs, so an Apple TV holds its picture back to match the door's sound (lip-sync).
# Built with `--target airplay2`. nqptp needs UDP 319/320 to itself, which the senders'
# airptpd holds on the host, so this runs on a LAN address of its own (a macvlan network,
# docker-compose.yml) and shares only the pipes volume with the hub. Both pinned to releases:
# shairport-sync 5.6 (the classic build's, AirPlay 2 from 4.0 on) and nqptp 1.2.9 (keeps one
# clock per receiver, which several AirPlay 2 receivers on one nqptp need).
FROM alpine:3.23 AS airplay2-build
ARG SHAIRPORT_VERSION=5.6
ARG NQPTP_VERSION=1.2.9
RUN apk add -U -q --no-cache --no-progress \
      autoconf automake avahi-dev build-base ffmpeg-dev git libconfig-dev libgcrypt-dev \
      libplist-dev libplist-util libsodium-dev libtool linux-headers openssl-dev pkgconf \
      popt-dev util-linux-dev xxd
RUN git clone --depth 1 -b "$NQPTP_VERSION" https://github.com/mikebrady/nqptp.git /tmp/nqptp \
    && cd /tmp/nqptp \
    && autoreconf -fi \
    && ./configure --prefix=/usr \
    && make \
    && install -D nqptp /tmp/build/usr/bin/nqptp
RUN git clone --depth 1 -b "$SHAIRPORT_VERSION" https://github.com/mikebrady/shairport-sync.git /tmp/shairport \
    && cd /tmp/shairport \
    && autoreconf -fi \
    && ./configure --with-airplay-2 --with-pipe --with-avahi --with-ssl=openssl \
         --prefix=/usr --sysconfdir=/etc \
    && make \
    && install -D shairport-sync /tmp/build/usr/bin/shairport-sync

FROM alpine:3.23 AS airplay2
RUN apk add -U -q --no-cache --no-progress \
      avahi dbus ffmpeg-libavcodec ffmpeg-libavformat ffmpeg-libavutil ffmpeg-libswresample \
      libconfig libgcrypt libplist libsodium libuuid openssl popt su-exec \
    && rm -f /etc/avahi/services/* \
    && adduser -S -H -D shairport
COPY --from=airplay2-build /tmp/build/ /
# It must load and be the AirPlay 2 build: a library missing here fails the build, not the
# first cast.
RUN shairport-sync -V | tee /dev/stderr | grep -q AirPlay2
COPY --chmod=755 scripts/sound-airplay2.sh /usr/local/bin/sound-airplay2
ENTRYPOINT ["/usr/local/bin/sound-airplay2"]

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

# The lab's checks, built once for the workbench and the bench (ADR 0115, 0123).
FROM debian:bookworm-slim AS lab-checks
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates python3 python3-venv \
    && rm -rf /var/lib/apt/lists/*
# The lab's checks (ADR 0115), as the fork's CI runs them, in a venv of their own: ansible-core
# (the engine alone; the `ansible` package is hundreds of collections the checks never load, and
# minutes more of arm64 build), the fork's CI pins of ansible-lint and yamllint, and the
# collections and roles the fork's requirements.yml names, baked in because the sandbox cannot
# fetch them: `--syntax-check` resolves every module, and ansible-lint runs `--offline`. No docs,
# no byte-code, no caches: the image stays small.
ARG ANSIBLE_CORE_VERSION=2.15.13
ARG ANSIBLE_LINT_VERSION=6.14.6
ARG YAMLLINT_VERSION=1.30.0
RUN python3 -m venv /opt/venv \
    && /opt/venv/bin/pip install --no-cache-dir --no-compile \
        "ansible-core==${ANSIBLE_CORE_VERSION}" \
        "ansible-lint==${ANSIBLE_LINT_VERSION}" \
        "yamllint==${YAMLLINT_VERSION}" \
    && /opt/venv/bin/ansible-galaxy collection install -p /opt/venv/collections \
        ansible.posix community.docker community.general vladgh.samba \
    && /opt/venv/bin/ansible-galaxy role install -p /opt/venv/roles \
        geerlingguy.docker,7.1.0 geerlingguy.nfs,2.0.0 \
    && find /opt/venv -type d -name '__pycache__' -prune -exec rm -rf '{}' + \
    && rm -rf /root/.ansible /root/.cache /opt/venv/collections/ansible_collections/*/*/docs \
    && chmod -R a+rX /opt/venv
# Endora's own collection (endora.homelab), built from THIS source so the image carries the
# version of the Endora it was built from. The fork's requirements.yml pins it to a git commit,
# which the sandbox cannot fetch; the baked copy is what `--syntax-check` and ansible-lint find.
# (`ansible-doc -t role -l` lists only roles with an argument spec, so the smoke checks look for
# the role files themselves.) `--no-deps`: its one dependency, community.docker, is installed above.
COPY deploy/ansible/collection /tmp/endora-collection
RUN /opt/venv/bin/ansible-galaxy collection build --output-path /tmp/endora-build /tmp/endora-collection \
    && /opt/venv/bin/ansible-galaxy collection install --no-deps -p /opt/venv/collections \
        /tmp/endora-build/endora-homelab-*.tar.gz \
    && /opt/venv/bin/ansible-galaxy collection list -p /opt/venv/collections endora.homelab \
        | grep -q endora.homelab \
    && test -f /opt/venv/collections/ansible_collections/endora/homelab/roles/mail_mirror/tasks/main.yml \
    && rm -rf /tmp/endora-collection /tmp/endora-build /root/.ansible \
    && chmod -R a+rX /opt/venv

# Claude Code, one pinned release, for the workbench and the bench (ADR 0117, 0123).
FROM debian:bookworm-slim AS claude-code
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
# Claude Code (ADR 0117), a developer's runtime when the owner chooses it: the native binary of
# one pinned release, fetched at build time from Anthropic's release bucket and checked against
# the digest pinned here (the release manifest's, per architecture); the sandbox never fetches it,
# never updates it, and holds no credential for it. 2.1.293 is the first that knows Haiku 5.5,
# 2.1.295 the first whose hooks can block; this one is a week older than the newest.
ARG CLAUDE_CODE_VERSION=2.1.296
ARG CLAUDE_CODE_SHA256_ARM64=sha256:f1f6e96e0d8342b9dbf41d7e88255397a6a52ce3d8736ad6a4c6b59c9b62fefa
ARG CLAUDE_CODE_SHA256_AMD64=sha256:24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de
ARG TARGETARCH
RUN set -eu; \
    case "$TARGETARCH" in \
      arm64) platform=linux-arm64; sum="$CLAUDE_CODE_SHA256_ARM64" ;; \
      amd64) platform=linux-x64; sum="$CLAUDE_CODE_SHA256_AMD64" ;; \
      *) echo "no Claude Code for $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /usr/local/bin/claude \
      "https://downloads.claude.ai/claude-code-releases/${CLAUDE_CODE_VERSION}/${platform}/claude"; \
    echo "${sum#sha256:}  /usr/local/bin/claude" | sha256sum -c -; \
    chmod 755 /usr/local/bin/claude

# The workbench (ring 1): a team member's disposable sandbox on the Pi, one container a turn,
# where the worker gives the model a real shell. Built with `--target workbench` and tagged
# endora-workbench:latest:
#   docker buildx build --platform linux/arm64 --target workbench -t endora-workbench:latest --load .
# It is not the node's image: the node keeps the last stage. Run with a read-only root and /work
# as a tmpfs owned by the workbench user. The sandbox is offline (ADR 0110): it reaches the
# node's team gateway and nothing else, so everything a turn may need is in this image; the
# egress proxy is designed and deferred. The same image runs the wing's check jobs (ADR 0115,
# ENDORA_CHECK=1): no model, a job token, the lab's own checks on a submitted branch.
FROM debian:bookworm-slim AS workbench
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash ca-certificates curl dnsutils git iputils-ping jq less procps python3 python3-venv \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --home-dir /work --no-create-home --shell /bin/bash workbench
# The lab's checks and Claude Code, from their stages above (ADR 0115, 0117).
COPY --from=lab-checks /opt/venv /opt/venv
COPY --from=claude-code /usr/local/bin/claude /usr/local/bin/claude
# Every shell in the sandbox is a login shell (`bash -lc`: a member's shell, Claude Code's
# Bash, the check job), and Debian's /etc/profile resets PATH, dropping the image's own. Put
# the lab's tools back for all of them, so no member is told they are missing.
RUN printf 'export PATH=/opt/venv/bin:$PATH\n' > /etc/profile.d/endora-lab-tools.sh \
    && bash -lc 'command -v ansible-playbook ansible-lint yamllint ansible-galaxy' >/dev/null
# Fail the build, not the first check job, if a check tool did not make it across the COPY.
RUN /opt/venv/bin/ansible-playbook --version && /opt/venv/bin/ansible-lint --version \
    && /opt/venv/bin/yamllint --version \
    && ANSIBLE_COLLECTIONS_PATH=/opt/venv/collections /opt/venv/bin/ansible-galaxy collection list endora.homelab \
        | grep -q endora.homelab \
    && test -f /opt/venv/collections/ansible_collections/endora/homelab/roles/endora_terminal_agent/tasks/main.yml
# Its settings, read-only and above any other (managed): no updates, telemetry, error reports,
# feedback or other traffic but the model's; no experimental betas; the five-minute cache the
# relay meters; no attribution of its own (the wing's commits carry Endora's trailer); the web
# tools refused; the hook that reports each of its own tools to the node as a
# sandbox-reported receipt; and the hook that, after a compaction, gives the turn the map its
# briefing opened with.
# (The folder is made first: one made by COPY --chmod takes the file's mode, and Claude Code,
# not root, could then not open it, and would run on none of these settings, silently.)
RUN install -d -m 755 /etc/claude-code
COPY --chmod=444 deploy/workbench/managed-settings.json /etc/claude-code/managed-settings.json
COPY --from=builder /endora-worker /usr/local/bin/endora-worker
ENV HOME=/work
# Claude Code keeps its state in the turn's own /work (a tmpfs, gone with the turn), and is
# told the same as its settings say before it reads them.
ENV CLAUDE_CONFIG_DIR=/work/.claude
ENV DISABLE_AUTOUPDATER=1 DISABLE_UPDATES=1 DISABLE_TELEMETRY=1 DISABLE_ERROR_REPORTING=1 \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 \
    CLAUDE_CODE_PROMPT_CACHE_TTL=5m
ENV ENDORA_WORKBENCH=1
ENV PATH=/opt/venv/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ENV ANSIBLE_COLLECTIONS_PATH=/opt/venv/collections
ENV ANSIBLE_ROLES_PATH=/opt/venv/roles
ENV ANSIBLE_LOCAL_TEMP=/tmp/ansible
ENV ANSIBLE_REMOTE_TEMP=/tmp/ansible
WORKDIR /work
USER workbench
ENTRYPOINT ["endora-worker"]

# The bench (ADR 0123): an external agent harness, fenced as a turn is, run once on one real task
# to measure it against HQ's own wing. Built with `--target bench` and a named build context
# `harness`, a folder holding `harness.tgz`, the harness's npm package packed from one pinned
# commit (the Makefile's `bench-image`; the commit is kept as a label). Its packages are fetched
# at build time; the bench itself, like the workbench, reaches only the node's team gateway, and
# there only the model relay, with the bench's own token passed in at run time. The same lab
# checks and the same Claude Code as the workbench, with settings of its own: no hook (the bench
# is no turn of HQ's), the workbench's shell allowlist, and bypass mode refused.
FROM node:22.22.1-bookworm-slim AS bench-install
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates g++ make python3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=harness harness.tgz /tmp/harness.tgz
RUN npm install -g --prefix /opt/harness --no-audit --no-fund /tmp/harness.tgz \
    && rm -rf /root/.npm /tmp/harness.tgz

FROM node:22.22.1-bookworm-slim AS bench
ARG BENCH_HARNESS_COMMIT=unknown
LABEL org.endora.bench.harness-commit=$BENCH_HARNESS_COMMIT
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash ca-certificates curl git jq less procps python3 tini tmux \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --home-dir /work --no-create-home --shell /bin/bash bench
COPY --from=lab-checks /opt/venv /opt/venv
COPY --from=claude-code /usr/local/bin/claude /usr/local/bin/claude
# Every shell in the sandbox is a login shell (`bash -lc`: a member's shell, Claude Code's
# Bash, the check job), and Debian's /etc/profile resets PATH, dropping the image's own. Put
# the lab's tools back for all of them, so no member is told they are missing.
RUN printf 'export PATH=/opt/venv/bin:$PATH\n' > /etc/profile.d/endora-lab-tools.sh \
    && bash -lc 'command -v ansible-playbook ansible-lint yamllint ansible-galaxy' >/dev/null
# Fail the build, not the first check job, if a check tool did not make it across the COPY.
RUN /opt/venv/bin/ansible-playbook --version && /opt/venv/bin/ansible-lint --version \
    && /opt/venv/bin/yamllint --version \
    && ANSIBLE_COLLECTIONS_PATH=/opt/venv/collections /opt/venv/bin/ansible-galaxy collection list endora.homelab \
        | grep -q endora.homelab \
    && test -f /opt/venv/collections/ansible_collections/endora/homelab/roles/endora_terminal_agent/tasks/main.yml
COPY --from=bench-install /opt/harness /opt/harness
RUN install -d -m 755 /etc/claude-code
COPY --chmod=444 deploy/bench/managed-settings.json /etc/claude-code/managed-settings.json
ENV HOME=/work
ENV CLAUDE_CONFIG_DIR=/work/.claude
ENV DISABLE_AUTOUPDATER=1 DISABLE_UPDATES=1 DISABLE_TELEMETRY=1 DISABLE_ERROR_REPORTING=1 \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 \
    CLAUDE_CODE_PROMPT_CACHE_TTL=5m
ENV PATH=/opt/harness/bin:/opt/venv/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ENV ANSIBLE_COLLECTIONS_PATH=/opt/venv/collections
ENV ANSIBLE_ROLES_PATH=/opt/venv/roles
ENV ANSIBLE_LOCAL_TEMP=/tmp/ansible
ENV ANSIBLE_REMOTE_TEMP=/tmp/ansible
WORKDIR /work
USER bench
# tini reaps what the harness's terminal sessions leave behind.
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["sleep", "infinity"]

# --- runtime stage ---------------------------------------------------------
FROM debian:bookworm-slim
# ca-certificates lets an optional HTTPS model endpoint be reached; SQLite is
# statically linked. nodejs+npm (for `npx …` servers) and python3 (for python
# servers) are the two runtimes the vast majority of stdio MCP servers need
# (ADR 0021) — without them a registered server's command isn't found and it stays
# unconnected. Node's `npx` fetches a package on first run, so a freshly registered
# npx server may need one retry while it downloads; a globally pre-installed server
# (or one already cached) connects immediately.
# openssh-client: the wing's approved branches are pushed to the owner's fork over SSH with a
# deploy key (ADR 0115); git clones the fork's mirror over plain HTTPS.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates git nodejs npm openssh-client python3 \
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
