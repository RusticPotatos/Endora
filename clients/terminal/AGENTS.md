# Working agreements

This is the handheld terminal client, `clients/terminal` in the Endora repository (ADR 0099);
the repository's own AGENTS.md applies too. The terminal speaks to the node only over its
HTTP API and imports no server crates; a change on both sides lands in one branch, and the
repository's checks build and test both. Branch from develop.

Use test-first behavior changes, Conventional Commits, and fail-loud file edits.
No speculative dependencies, server internals, or execution of model-generated
commands. Real configuration, credentials, addresses, SSIDs, device backups and
diagnostic captures stay outside Git. Use fictional examples and ignored
privacy.local to extend local checks. Never print matched secret values.

Before handoff run Rust fmt/check, Clippy with warnings denied, all tests, Python
developer tests, scripts/check-private.py, scripts/check-pty.py against the native
binary, and git diff --check. Test installation on Debian ARM64 before claiming
support. State which device and live-server checks actually ran.

Do not push, publish releases or open PRs without explicit instructions. Disclose
AI assistance in any requested PR. Boot changes must preserve SSH, NetworkManager,
working drivers and a recovery path. Private boot backups never belong in Git.
