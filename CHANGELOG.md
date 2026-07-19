# Changelog

All notable changes to Endora are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a
tagged release.

## [Unreleased]

Endora is in its foundation phase; there is no tagged release yet. This section
captures what has been built toward the first one.

### Added

- **Project foundation** — constitution, architecture docs, ADRs (0001–0008),
  contribution/governance/security policies, and a domain-first modular-monolith
  Rust workspace (`endora-domain`, `endora-application`, `endora-infrastructure`,
  `endora-node`, `endora-cli`).
- **The learning loop, end to end** — Direction → Goal → Assumption → Experiment
  (with a proposed/running/concluded lifecycle) → Observation → Reflection (over
  cited evidence) → Proposed process change, all persisted (SQLite) and served
  over a versioned HTTP/JSON protocol, with a CLI client.
- **Deterministic policy boundary** — models propose; deterministic policy
  authorizes. Enacting a process change requires explicit human approval and an
  autonomy-appropriate actor; the decision endpoint returns permit /
  require-human-approval / deny.
- **Audit trail** — every consequential policy decision is recorded and readable
  via `GET /v1/audit`.
- **Local model adapter** — an optional local, OpenAI-compatible model (e.g.
  Qwen3.5 via Ollama) can *draft* a process change from a reflection; the draft
  is an ordinary pending proposal that still passes through the policy boundary.
  The node degrades gracefully (HTTP 503) when no model is available.
- **Memory rights** — `GET /v1/export` returns all of a user's data as JSON;
  `POST /v1/memory/purge` (with explicit confirmation) permanently deletes it.
- **Packaging** — a multi-stage `Dockerfile`; the node targets macOS, Linux,
  Ubuntu Server, and Docker.
- **Tooling** — a `Makefile` (bootstrap, run, dev, `ci`), GitHub Actions CI
  (fmt, Clippy with warnings denied, tests on Linux and macOS), and Dependabot.

[Unreleased]: https://github.com/RusticPotatos/Endora/commits/develop
