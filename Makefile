# Endora developer task runner.
#
# Run `make` (or `make help`) to list targets. Every target is a thin wrapper
# around the real cargo/rustup commands, so nothing here hides what actually
# runs. First time on a machine: `make bootstrap`, then `make ci`.

CARGO ?= cargo
# Flags applied to workspace-wide cargo invocations. Override on the command
# line, e.g. `make test WORKSPACE_FLAGS=--workspace`.
WORKSPACE_FLAGS ?= --workspace --all-features

# Optional per-machine settings, kept out of git: put `DOCKER_CONTEXT = nas`
# (or any other override) in `local.mk` and every target below picks it up.
# Deployment targets are a property of *your* machine, not of the project, so
# they belong here rather than baked into a default everyone inherits.
-include local.mk

# Where the Compose deploy targets run. Empty = the local Docker daemon, which
# is what a fresh clone should do: `make deploy` on a new machine must work
# self-contained, without assuming a host that only exists on one network. Set a
# remote context to build+run there, e.g. `make deploy DOCKER_CONTEXT=nas`, or
# once and for all in local.mk.
DOCKER_CONTEXT ?=
# Where `make smoke` looks for the deployed node. Override in local.mk to point at the
# host you actually deploy to; the default is a local `make deploy`.
ENDORA_URL ?= https://127.0.0.1:8787
# The credential `make smoke` signs its requests with (the node refuses /v1 without one).
# Put it in the git-ignored local.mk beside ENDORA_URL; without it the suite gets 401 and
# says so by name rather than looking like a broken screen.
#
# Use a CHECKING token — read-only, and refused /v1/export. Not the bootstrap token from the
# node's log, and not your console session either: this one lives in a plaintext file, so it
# should be the credential that cannot point the deep model somewhere else, widen a
# capability, purge memory, or pull the whole conversation in one call.
#
# Mint one from a signed-in console (devtools, on the Endora tab):
#
#     await (await fetch('/v1/session/checks', {method:'POST',
#       headers:{authorization:'Bearer '+localStorage.getItem('endora-token')}})).json()
#
# It can still read beliefs and context, which the suite asserts on — deliberate, not a gap.
ENDORA_TOKEN ?=

# Your own values — your name, your city, your postcode, anyone you live with — comma
# separated, so `make personal-check` can prove none of them are in the tree. They go in
# local.mk and nowhere else, which is the point rather than a precaution: a city, a county,
# a postcode and three people's names once reached this public repository through test
# fixtures, and every one of them was caught by somebody reading a diff.
#
# Empty here and empty in CI, deliberately. The machine that holds the real values is the
# only machine that can check for them; CI checks by shape instead.
ENDORA_PERSONAL_VALUES ?=

# Compose derives its project name from the working directory, so running a
# deploy from a git worktree would invent a NEW project — a fresh empty volume,
# and a name collision with the real deployment. Pinning it means every deploy
# targets the same containers and the same data wherever it is run from.
COMPOSE_PROJECT_NAME ?= endora
export COMPOSE_PROJECT_NAME

# Where the node keeps its data (/data in the container): the database, which holds every
# setting, API key and sign-in secret, and the TLS cert and key. Empty = a Docker named
# volume, the default and what a fresh clone wants. Set an ABSOLUTE path in local.mk to keep
# it in a folder your backup tool can reach — a path on the machine the Docker daemon runs
# on, so on a NAS it is a path on the NAS, on a local disk and not a network share (SQLite's
# locking is not reliable over one). Existing data is copied across once with
# `make migrate-data`; see docs/hosting.md.
ENDORA_DATA_DIR ?=
export ENDORA_DATA_DIR

# ENDORA_TIME_ZONE (e.g. America/New_York) in local.mk, so the model is told your local time and
# the brief keeps its hour when the clocks change. ENDORA_UTC_OFFSET_MINUTES (e.g. -300) is a
# fixed fallback. UTC when unset.
export ENDORA_TIME_ZONE
export ENDORA_UTC_OFFSET_MINUTES

# What Endora knows about you (ADR 0101): markdown pages with their own git history, at
# /brain in the container. ENDORA_BRAIN_DIR = <docker home>/endora/brain in local.mk keeps them
# in a host folder you can open; unset, a named volume.
export ENDORA_BRAIN_DIR

# Endora HQ (ADR 0102), the team service: COMPOSE_PROFILES = hq and
# ENDORA_HQ_URL = http://hq:8790 in local.mk run it beside the node, with the scripted demo team
# unless ENDORA_HQ_DEMO = 0. It makes its own token for the node; nothing to keep by hand.
# ENDORA_HQ_TICK_SECONDS sets how often its engine ticks (30 unless set).
export COMPOSE_PROFILES
export ENDORA_HQ_URL
export ENDORA_HQ_DEMO
export ENDORA_HQ_TICK_SECONDS
export ENDORA_HQ_DEMO
export ENDORA_HQ_TICK_SECONDS

# The read-only mail copy on the NAS (ansible-nas `mail_mirror` role, ADR 0097), e.g.
# ENDORA_MAIL_DIR = /srv/mail-mirror/maildir in local.mk. Read whenever the copy is there.
export ENDORA_MAIL_DIR

COMPOSE = docker $(if $(DOCKER_CONTEXT),--context $(DOCKER_CONTEXT),) compose
TARGET_DOCKER = docker $(if $(DOCKER_CONTEXT),--context $(DOCKER_CONTEXT),)

# Where the image is BUILT, when that is not where it runs: a Docker context on this machine
# (e.g. ENDORA_BUILD_CONTEXT = desktop-linux in local.mk on a laptop). The image is built
# there for the target's architecture, cross-compiled rather than emulated, and copied to
# the target, which then only starts it. Empty = build on the target, as before. A small
# always-on box shares its memory with everything else in the house: a release build on one
# ran a 16 GB NAS out of memory and swap, and Home Assistant stopped answering.
ENDORA_BUILD_CONTEXT ?=

.DEFAULT_GOAL := help

## ----------------------------------------------------------------------------
## Setup
## ----------------------------------------------------------------------------

.PHONY: bootstrap
bootstrap: ## Install the toolchain + components and pre-fetch dependencies
	@command -v rustup >/dev/null 2>&1 || { \
		echo "rustup not found. Install it from https://rustup.rs/ then re-run 'make bootstrap'."; \
		exit 1; }
	rustup show
	rustup component add rustfmt clippy
	$(CARGO) fetch
	@echo "Bootstrap complete. Next: make ci"

## ----------------------------------------------------------------------------
## Build & run
## ----------------------------------------------------------------------------

.PHONY: build
build: ## Compile the whole workspace
	$(CARGO) build $(WORKSPACE_FLAGS)

.PHONY: check
check: ## Type-check the workspace without producing binaries
	$(CARGO) check $(WORKSPACE_FLAGS)

.PHONY: run-node
run-node: ## Run the authoritative node (HTTP server; ENDORA_ADDR/ENDORA_DB to configure)
	$(CARGO) run --bin endora-node

.PHONY: run-cli
run-cli: ## Run the CLI client (pass args via ARGS="...", e.g. ARGS="health")
	$(CARGO) run --bin endora -- $(ARGS)

.PHONY: demo
demo: ## Run the full learning loop against a throwaway node (release build)
	@$(CARGO) build --release -q
	@db=$$(mktemp -t endora-demo.XXXXXX); port=8799; \
	ENDORA_DB=$$db ENDORA_ADDR=127.0.0.1:$$port ./target/release/endora-node >/dev/null 2>&1 & \
	node=$$!; \
	trap 'kill $$node 2>/dev/null; rm -f $$db' EXIT; \
	for i in $$(seq 1 30); do curl -fsS http://127.0.0.1:$$port/health >/dev/null 2>&1 && break; sleep 0.2; done; \
	ENDORA=./target/release/endora ENDORA_URL=http://127.0.0.1:$$port ./scripts/demo.sh

.PHONY: docker-build
docker-build: ## Build the node container image (tag: endora-node)
	docker build -t endora-node .

.PHONY: docker-run
docker-run: ## Run the node container (loopback-only on 8787, persists ./endora-data)
	# Bind the published port to loopback: authentication does not encrypt transport, so
	# it must not be reachable off this machine. See docs/hosting.md to reach it
	# securely from other devices.
	docker run --rm -p 127.0.0.1:8787:8787 -v "$(CURDIR)/endora-data:/data" endora-node

.PHONY: check-data-dir
check-data-dir:
	@DOCKER_CONTEXT="$(DOCKER_CONTEXT)" sh scripts/data-dir.sh check

.PHONY: deploy
deploy: check-data-dir ## Build + start the node via Compose (DOCKER_CONTEXT=nas to target a remote host)
	# Builds on the target host and starts it detached; data persists in the
	# named volume `$(COMPOSE_PROJECT_NAME)_endora-data`, or in ENDORA_DATA_DIR when that
	# is set (check-data-dir refuses to start on an empty one while the volume still
	# holds the node's data — that would be a brand-new node). Runs against the local
	# Docker daemon unless DOCKER_CONTEXT names a remote one (set it per-machine
	# in local.mk). Keep the bearer-authenticated API on a trusted network.
	# ENDORA_BUILD stamps the deploy's git short SHA into the image, so /health
	# and the console header show which build is live.
ifeq ($(ENDORA_BUILD_CONTEXT),)
	ENDORA_BUILD="$$(git rev-parse --short HEAD 2>/dev/null || echo dev)" $(COMPOSE) up -d --build
else
	@set -e; \
	build="$$(git rev-parse --short HEAD 2>/dev/null || echo dev)"; \
	arch="$$($(TARGET_DOCKER) version --format '{{.Server.Arch}}')"; \
	echo "building $$build for linux/$$arch on $(ENDORA_BUILD_CONTEXT)"; \
	docker --context $(ENDORA_BUILD_CONTEXT) buildx build --platform "linux/$$arch" \
		--build-arg ENDORA_BUILD="$$build" -t endora-node:for-target --load .; \
	echo "copying the image to the target"; \
	docker --context $(ENDORA_BUILD_CONTEXT) save endora-node:for-target | gzip -1 | $(TARGET_DOCKER) load; \
	$(TARGET_DOCKER) tag endora-node:for-target endora-node:latest; \
	ENDORA_BUILD="$$build" $(COMPOSE) up -d --no-build
endif

.PHONY: smoke
smoke: ## Assert invariants against the DEPLOYED node (ENDORA_URL, or https://127.0.0.1:8787)
	# The tier CI cannot run: GitHub cannot reach your house. Run it after `make deploy`.
	# It asserts about the live instance's real data using the production rules, which is
	# where five of the last six bugs in this system were visible within a minute of the
	# deploy that introduced them — nobody was looking. Set ENDORA_URL in local.mk to
	# point at the deployed host.
	@ENDORA_URL="$(ENDORA_URL)" ENDORA_TOKEN="$(ENDORA_TOKEN)" cargo test -p endora-infrastructure --test live_smoke -- --ignored --test-threads=1

.PHONY: deploy-check
deploy-check: deploy ## Deploy, wait for the node to come up, then smoke it
	@printf 'waiting for %s' "$(ENDORA_URL)"; \
	for i in $$(seq 1 30); do \
		if curl -sk --max-time 3 "$(ENDORA_URL)/health" >/dev/null 2>&1; then echo " up"; break; fi; \
		printf '.'; sleep 2; \
	done
	$(MAKE) smoke

.PHONY: bundled
bundled: ## Start Endora WITH a bundled model runtime — no Ollama, nothing to install first
	# For someone who wants it working before they want to configure anything. The runtime
	# container looks at the hardware it can see and serves a model that suits it; Endora
	# just talks to the URL, exactly as it would to your own endpoint (ADR 0055).
	#
	# First start downloads a model (1–5 GB depending on what it finds) into a named
	# volume, so it happens once.
	ENDORA_MODEL_URL=http://runtime:8080/v1 ENDORA_MODEL=bundled \
	ENDORA_ROUTER_MODEL= ENDORA_SYNTH_MODEL= \
	ENDORA_BUILD="$$(git rev-parse --short HEAD 2>/dev/null || echo dev)" \
	$(COMPOSE) --profile bundled up -d --build

.PHONY: deploy-logs
deploy-logs: ## Follow the deployed node's logs (respects DOCKER_CONTEXT)
	$(COMPOSE) logs -f

.PHONY: deploy-down
deploy-down: ## Stop the deployed node, keeping its data volume (respects DOCKER_CONTEXT)
	$(COMPOSE) down

.PHONY: migrate-data
migrate-data: ## One-time: copy the data volume into ENDORA_DATA_DIR (stop the node first; deletes nothing)
	@DOCKER_CONTEXT="$(DOCKER_CONTEXT)" sh scripts/data-dir.sh migrate

.PHONY: backup
backup: ## Snapshot the running node's database (settings, keys, memory) into its data dir
	# A consistent copy taken through SQLite while the node runs — not a file copy, which can
	# land mid-write. It goes to <data dir>/backups/, so whatever backs that folder up picks it
	# up, and it is written owner-only (0600) because it holds the node's credentials. Old
	# snapshots are not pruned; that is the backup tool's job. Respects DOCKER_CONTEXT.
	#
	# The first line refuses if the RUNNING binary predates the subcommand: an older node
	# ignores `backup` as an argument and would start a second server inside the container.
	@$(COMPOSE) exec -T node grep -qa 'usage: endora-node backup' /usr/local/bin/endora-node \
		|| { echo "The running node predates 'endora-node backup'. Deploy this version first: make deploy"; exit 1; }
	$(COMPOSE) exec -T node endora-node backup /data/backups/endora-$$(date +%Y%m%d-%H%M%S).db

.PHONY: data-dir-test
data-dir-test: ## Test the data-dir guard and migration against a fake docker (no daemon needed)
	sh scripts/test-data-dir.sh

.PHONY: watch
watch: ## Re-run tests on file change (needs cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { \
		echo "cargo-watch not installed. Run: cargo install cargo-watch"; \
		exit 1; }
	$(CARGO) watch -x "test $(WORKSPACE_FLAGS)"

## ----------------------------------------------------------------------------
## Development: format, lint, test
## ----------------------------------------------------------------------------

.PHONY: foundation-gate
foundation-gate: ## Run foundation acceptance; missing coverage or failed behavior blocks release
	python3 -m unittest discover -s scripts/foundation -p 'test_*.py'
	python3 scripts/foundation/gate.py

.PHONY: evals
evals: ## Run the live model evaluations and keep a dated record (target/evals/); see scripts/run-live-evals.sh
	@ENDORA_MODEL_URL="$(ENDORA_MODEL_URL)" ENDORA_MODEL="$(ENDORA_MODEL)" ./scripts/run-live-evals.sh

.PHONY: goal-eval
goal-eval: ## Can it actually accomplish a goal? Drives the real pass loop with the real model
	# Live acceptance of the pass loop using production schema, reversible skills,
	# and a real model. Partial is not successful completion. This does not test
	# worker leases, scheduling, browser progress, or chat publication.
	#
	# Set ENDORA_MODEL_URL (OpenAI-compatible, e.g. your Ollama) and ENDORA_MODEL
	# in local.mk. Nothing here touches the deployed node or any personal data.
	@ENDORA_MODEL_URL="$(ENDORA_MODEL_URL)" ENDORA_MODEL="$(ENDORA_MODEL)" \
		cargo test -p endora-infrastructure --test goal_eval -- --ignored --nocapture --test-threads=1

.PHONY: console-check
console-check: ## Render every console screen in Node (catches a broken UI before a phone does)
	# The Rust half is checked by the compiler; the console had nothing. A call to a
	# function that no longer existed passed `node --check`, passed CI, deployed, and
	# rendered a blank page. This loads app.js and actually calls every screen.
	node scripts/check-console.mjs

.PHONY: personal-check
personal-check: ## Fail if anything personal is in the tree (a city and two names once were)
	# By shape — a real mailbox, a home address, coordinates, a phone number, a live
	# secret — so it does not need to know who you are. What it cannot guess, your own
	# name and your own city, comes from ENDORA_PERSONAL_VALUES in the git-ignored
	# local.mk. That is the rule stated as a mechanism: personal values are
	# configuration, and configuration is not in the repository.
	@ENDORA_PERSONAL_VALUES="$(ENDORA_PERSONAL_VALUES)" node scripts/check-nothing-personal.mjs

.PHONY: personal-check-published
personal-check-published: ## Also check what this repo has PUBLISHED — PR bodies, issues, comments
	# Rewriting history cannot reach a pull request's description. A city name survived a
	# tree scrub and two force-pushes sitting in a merged PR body, and nine more issues and
	# PRs were carrying names, a device and a host address. Needs the network and `gh`, so
	# it is not in `make ci`; run it after opening or editing anything.
	@ENDORA_PERSONAL_VALUES="$(ENDORA_PERSONAL_VALUES)" node scripts/check-nothing-personal.mjs --github

.PHONY: fmt
fmt: ## Format the code in place
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying files
	$(CARGO) fmt --all -- --check

.PHONY: clippy
clippy: ## Lint with Clippy, denying warnings
	$(CARGO) clippy $(WORKSPACE_FLAGS) --all-targets -- -D warnings

.PHONY: test
test: ## Run the test suite
	$(CARGO) test $(WORKSPACE_FLAGS)

.PHONY: doc
doc: ## Build API docs for the workspace
	$(CARGO) doc $(WORKSPACE_FLAGS) --no-deps

.PHONY: diff-check
diff-check: ## Fail on whitespace errors / leftover conflict markers
	git diff --check

.PHONY: ci
ci: fmt-check clippy test console-check personal-check diff-check ## Run every check exactly as CI does
	@echo "All CI checks passed."

## ----------------------------------------------------------------------------
## Housekeeping
## ----------------------------------------------------------------------------

.PHONY: clean
clean: ## Remove build artifacts (target/)
	$(CARGO) clean

.PHONY: help
help: ## Show this help
	@echo "Endora — make targets:"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "} {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "First time here? Run 'make bootstrap', then 'make ci'."
