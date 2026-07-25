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
ci: fmt-check clippy test diff-check ## Run every check exactly as CI does
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
