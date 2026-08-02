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
# Use a SESSION token, not the bootstrap one printed to the node's log. The bootstrap token
# never expires and cannot be rotated without editing the database; a session ages out in
# thirty days and is revoked by clearing `node_sessions`. A plaintext file on a laptop should
# hold the credential you can throw away.
#
# You already have one after signing in — in the browser's devtools console, on the Endora
# tab:
#
#     localStorage.getItem('endora-token')
#
# Preferred over signing in with `curl`, which would leave the password in shell history.
ENDORA_TOKEN ?=

# Compose derives its project name from the working directory, so running a
# deploy from a git worktree would invent a NEW project — a fresh empty volume,
# and a name collision with the real deployment. Pinning it means every deploy
# targets the same containers and the same data wherever it is run from.
COMPOSE_PROJECT_NAME ?= endora
export COMPOSE_PROJECT_NAME

COMPOSE = docker $(if $(DOCKER_CONTEXT),--context $(DOCKER_CONTEXT),) compose

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
	# Bind the published port to loopback: the API is unauthenticated in 0.x, so
	# it must not be reachable off this machine. See docs/hosting.md to reach it
	# securely from other devices.
	docker run --rm -p 127.0.0.1:8787:8787 -v "$(CURDIR)/endora-data:/data" endora-node

.PHONY: deploy
deploy: ## Build + start the node via Compose (DOCKER_CONTEXT=nas to target a remote host)
	# Builds on the target host and starts it detached; data persists in the
	# named volume `$(COMPOSE_PROJECT_NAME)_endora-data`. Runs against the local
	# Docker daemon unless DOCKER_CONTEXT names a remote one (set it per-machine
	# in local.mk). The 0.x API is unauthenticated — keep it on a trusted network.
	# ENDORA_BUILD stamps the deploy's git short SHA into the image, so /health
	# and the console header show which build is live.
	ENDORA_BUILD="$$(git rev-parse --short HEAD 2>/dev/null || echo dev)" $(COMPOSE) up -d --build

.PHONY: smoke
smoke: ## Assert invariants against the DEPLOYED node (ENDORA_URL, or https://127.0.0.1:8787)
	# The tier CI cannot run: GitHub cannot reach your house. Run it after `make deploy`.
	# It asserts about the live instance's real data using the production rules, which is
	# where five of the last six bugs in this system were visible within a minute of the
	# deploy that introduced them — nobody was looking. Set ENDORA_URL in local.mk to
	# point at the deployed host.
	ENDORA_URL="$(ENDORA_URL)" ENDORA_TOKEN="$(ENDORA_TOKEN)" cargo test -p endora-infrastructure --test live_smoke -- --ignored --test-threads=1

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

.PHONY: watch
watch: ## Re-run tests on file change (needs cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { \
		echo "cargo-watch not installed. Run: cargo install cargo-watch"; \
		exit 1; }
	$(CARGO) watch -x "test $(WORKSPACE_FLAGS)"

## ----------------------------------------------------------------------------
## Development: format, lint, test
## ----------------------------------------------------------------------------

.PHONY: console-check
console-check: ## Render every console screen in Node (catches a broken UI before a phone does)
	# The Rust half is checked by the compiler; the console had nothing. A call to a
	# function that no longer existed passed `node --check`, passed CI, deployed, and
	# rendered a blank page. This loads app.js and actually calls every screen.
	node scripts/check-console.mjs

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
ci: fmt-check clippy test console-check diff-check ## Run every check exactly as CI does
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
