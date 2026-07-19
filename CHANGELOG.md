# Changelog

All notable changes to Endora are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a
tagged release.

## [Unreleased]

### Added

- **Preferences — the butler learns (finishing 0.7)**
  ([ADR 0010](docs/adr/0010-autonomy-model.md)): the butler records what it learns
  about you (taste, or explicit grants of authority) as **visible, correctable,
  deletable** memory, and feeds it back into its own context so it stops re-asking
  what it already knows — "learning is the accumulation of preferences." New
  `/v1/preferences` (create, list, delete), CLI `preference add|list|delete`, a
  console **Preferences** view, and a `remember_preference` proposal the butler can
  make (you confirm it). Preferences are part of export and cleared by purge. The
  local model butler is now live on the NAS via Ollama.
- **Adaptive attention (0.8)** ([ADR 0016](docs/adr/0016-adaptive-attention.md)):
  the butler surfaces what needs attention — due reviews, North Stars not yet filed
  under a value, and North Stars with no active target — via `GET /v1/attention`,
  most pressing first. Each item can be deferred (`POST /v1/attention/snooze`) with
  **exponential backoff**: every "not now" doubles how long it stays hidden (1, 2,
  4, … days), so a repeatedly-deferred item asks less and less. The console home
  shows a "Needs your attention" section with a per-item "Later". The ranking serves
  the person's stated values, not engagement.
- **The butler — conversational MVP (0.7)**
  ([ADR 0014](docs/adr/0014-the-butler-conversation-values-attention.md)): a chat
  surface where the butler **proposes** structured actions from a closed set
  (create value / North Star) and the person **confirms** each one — existing
  deterministic use cases execute; the model never acts on its own. Two brains: a
  `ScriptedButler` (offline, deterministic, the reliable fallback) and an
  `LlmButler` (a local OpenAI-compatible model, with a candid, explicitly
  anti-sycophantic prompt, falling back to scripted if unavailable). The
  conversation is persisted and included in export/purge. New `POST/GET /v1/chat`,
  CLI `chat "<message>"`, and a console chat panel with confirmable proposals.
- **Values layer (0.6)** ([ADR 0015](docs/adr/0015-identity-and-values-context.md)):
  a **`Value`** — the *why* a North Star serves (health, community, craft) — sits
  above North Stars: **Value → North Star → Target**. A North Star can be filed
  under a value (assigned by the person, never inferred). New `/v1/values` (create,
  list, delete) and `POST /v1/directions/{id}/value`; CLI `value create|list|delete`
  and `direction value <id> <value-id|none>`; the console home groups North Stars by
  value. Deleting a value in use is refused (re-file first). Existing databases gain
  a `values` table and a nullable `value_id` on open.
- **Artifact lifecycle (0.5)**: North Stars and Targets gain a lifecycle status —
  **active / achieved / abandoned / archived** — plus **delete**, across the domain,
  API, CLI, and console. Set status via `POST /v1/directions/{id}` and
  `POST /v1/targets/{id}` (`{"status":"achieved"}`); delete via `DELETE` on the same
  paths (refused while dependents exist — archive instead). CLI: `direction status`,
  `direction delete`, `target status`, `target delete`. The console shows status
  pills, per-item lifecycle actions, and hides archived items behind a toggle.
  Existing databases gain the `status` column (defaulting to `active`) on open.

- **Phone-usable console and a hosting guide** (0.4): the web console gains a
  mobile breakpoint that stacks forms and tightens the layout so the whole loop
  is usable at phone widths. New [docs/hosting.md](docs/hosting.md) covers running
  the node always-on (systemd / container restart), backups, and reaching it
  securely from other devices over a private overlay (e.g. Tailscale) or an
  authenticating reverse proxy.
- **Activity feed and live updates**
  ([ADR 0012](docs/adr/0012-activity-feed-and-change-stream.md)): a newest-first
  timeline of what has happened (`GET /v1/activity`, CLI `activity [limit]`),
  derived from the already-persisted observations and audited decisions. The node
  also exposes a server-sent change stream (`GET /v1/activity/stream`) that emits a
  `changed` signal after any successful write; the web console subscribes and
  refreshes its feed — and due-review banner — live, no reload needed.
- **Review scheduling** ([ADR 0011](docs/adr/0011-review-scheduling-reminders.md)),
  the first application of the autonomy model
  ([ADR 0010](docs/adr/0010-autonomy-model.md)): an experiment can carry a
  scheduled review time, and the system surfaces reviews that are due without
  acting on them. New protocol endpoints `POST /v1/experiments/{id}/review` and
  `GET /v1/reviews/due`; CLI `experiment review <id> <days>` and `reviews due`;
  and a web-console "Remind me" control plus a due-review banner on the home view.

### Changed

- **Renamed the second-tier concept `Goal` → `Target`**
  ([ADR 0013](docs/adr/0013-rename-goal-to-target.md)): a North Star's children are
  now **targets** (a concrete, measurable outcome), not goals. This is a
  **breaking protocol change** — `/v1/directions/{id}/goals` → `…/targets`,
  `/v1/goals/{id}/…` → `/v1/targets/{id}/…`, and the `goal_id` field → `target_id`
  — with the CLI (`target …`) and web console updated to match. Existing databases
  migrate automatically on open (the `goals` table and `goal_id` columns are
  renamed in place, no data loss).
- The `experiments` table gains a nullable `review_by_ms` column, added to
  existing databases by an automatic forward migration on open.
- `make docker-run` now publishes the node on loopback only
  (`127.0.0.1:8787:8787`) rather than all interfaces, matching the documented
  security posture (the `0.x` API is unauthenticated).

## [0.1.0] - 2026-07-19

First tagged release. Endora is in its foundation phase and is **not** a general
autonomous agent; interfaces may still change before 1.0.

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

[Unreleased]: https://github.com/RusticPotatos/Endora/compare/v0.1.0...develop
[0.1.0]: https://github.com/RusticPotatos/Endora/releases/tag/v0.1.0
