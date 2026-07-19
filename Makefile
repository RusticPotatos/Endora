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
run-node: ## Run the authoritative node (foundation-phase placeholder)
	$(CARGO) run --bin endora-node

.PHONY: run-cli
run-cli: ## Run the CLI client (foundation-phase placeholder)
	$(CARGO) run --bin endora

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
