# Endora developer task runner.
#
# Run `make` (or `make help`) to list targets. Every target is a thin wrapper
# around the real cargo/rustup commands, so nothing here hides what actually
# runs. First time on a machine: `make bootstrap`, then `make ci`.

CARGO ?= cargo
# Flags applied to workspace-wide cargo invocations. Override on the command
# line, e.g. `make test WORKSPACE_FLAGS=--workspace`.
WORKSPACE_FLAGS ?= --workspace --all-features

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
