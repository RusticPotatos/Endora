# Changelog

All notable changes to Endora are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a
tagged release.

## [Unreleased]

### Changed

- **Direction reset — intent-first understanding** (canonical:
  [docs/direction-reset.md](docs/direction-reset.md); decision:
  [ADR 0020](docs/adr/0020-intent-first-understanding-loop.md)): Endora had drifted into
  a goal tracker; it is now an **autonomous personal intelligence** whose job is to
  **understand intent**. It maintains a living **Understanding** — beliefs about you
  (intent, values, patterns, motivations, frustrations…), each with the **evidence**
  behind it and a **confidence**, formed by Endora itself and **reviewable/correctable**
  by you (no per-item confirmation). The **home screen** is now "what Endora understands
  about you," beliefs grouped by kind with *That's right / Not quite*; **goals are
  demoted to optional**. The butler is grounded in what it already understands (builds on
  it, avoids duplicates) and phrases beliefs in second person; check-ins draw on
  understanding. New `GET /v1/understanding` and affirm/correct; beliefs are in
  export/purge. *Interventions* (the butler acting on that understanding by using its
  skills) now land as the next layer — see Added.

### Faster builds

- The container build now uses **BuildKit cache mounts** for the cargo registry and
  `target/`, so deploys do an **incremental** compile (only changed crates) instead of
  rebuilding every dependency each time — a normal change goes from minutes to seconds
  after the first (cache-priming) build.

### Added

- **The butler actually uses its skills — interventions**: when answering needs
  current facts it doesn't have (weather, local safety alerts, a web page), the butler
  now **uses a skill and answers with the real result** instead of only saying it will.
  The model *proposes* a skill (`use`); a deterministic policy check authorizes it —
  only a **configured, read-only** skill runs on its own, anything consequential (e.g.
  flights) stays gated for you to confirm — then the skill executes and the butler
  answers using what came back (one tool round per turn). If it isn't cleared or the
  model asks for nothing, the reply is unchanged, so nothing regresses. The turn's
  activity notes which skill was used. This is the *interventions* layer promised by
  the direction reset ([ADR 0020](docs/adr/0020-intent-first-understanding-loop.md)),
  built on the capability registry ([ADR 0019](docs/adr/0019-proactive-self-improving-butler.md)).
- **See what Endora does — activity in the chat**: after each turn the chat shows a
  subtle note of what happened behind the scenes ("Learned that you find mornings
  hard", "Added to your inbox — …", "Grew more sure that …") — closure on a
  conversation and a window for debugging. Toggle it from the menu ("Show Endora's
  actions"). The chat responses now include an `activity` list.
- **A gentle "where are you based?" setup**: when Endora doesn't know your location,
  the home screen offers a one-line prompt; it's stored as a preference the butler
  already reads, so skills like weather and the guard dog have a starting point — no
  separate onboarding.
- **Capabilities — the butler's skills** (third slice of
  [ADR 0019](docs/adr/0019-proactive-self-improving-butler.md)): pluggable modules the
  butler can reach for, each declaring what it does, its **autonomy level** (act vs
  ask), whether it **leaves the device**, and whether it's **configured**. Working
  today (no keys): **Weather** (current + today + a severe-weather heads-up),
  **Web browsing** (fetch a page, read its text), and a **Guard dog** (active
  public-safety alerts via the US National Weather Service). Declared and awaiting
  setup: image review (local vision model), local events, flight search, location
  tracking, incident scanner — each shown with what it needs, not silently missing.
  New `GET /v1/capabilities` and `POST /v1/capabilities/{id}/invoke`; a **Skills**
  view in the console. (The registry is the substrate an MCP host adapter plugs into.)
- **The butler reaches out — proactive check-ins** (second slice of
  [ADR 0019](docs/adr/0019-proactive-self-improving-butler.md)): the node now runs a
  **heartbeat**, and on a cadence you choose (a **Check-ins** control in the chat —
  off by default, or every couple of minutes / hourly / daily) the butler posts a
  proactive opening message grounded in what needs attention and what you're working
  toward, always asking how it can serve you better — the self-improvement loop, in
  conversation. It only ever posts a *message*; anything consequential still goes
  through propose→confirm (the autonomy model). New `GET/POST /v1/checkin`; the
  cadence is cleared by purge.
- **Chat learnings persist — a suggestion inbox** (first slice of
  [ADR 0019](docs/adr/0019-proactive-self-improving-butler.md)): the butler's
  proposals used to vanish on reload unless you confirmed them in the moment. Now
  every proposal is saved as a durable **suggestion** tied to the reply it came
  from, and collects in an **Inbox** (new `GET /v1/suggestions`, with a count on the
  nav) where you can **Apply** or **Dismiss** it any time — applying runs the same
  deterministic create, so it flows into your North Stars / targets / preferences
  (you authorize; the butler only proposed). Suggestions are part of export and
  cleared by purge. **Targets now attach correctly**: the North Star a target names
  is resolved when applied — by id, or by matching the name to an existing North
  Star — instead of the proposal being silently dropped when the model gave a name
  rather than an id (`POST /v1/suggestions/{id}/apply|dismiss`).
- **The butler's reply streams in live, token-by-token**
  ([ADR 0018](docs/adr/0018-streaming-chat-responses.md)): a new
  `POST /v1/chat/stream` (Server-Sent Events) streams the reply's prose as the model
  produces it, so the console grows the butler's bubble word-by-word instead of
  waiting for the whole thing — the big difference in *feel* between a spinner and a
  live agent. The structured proposals still arrive at the end and are still
  confirmed by you (the model never acts). Built on the existing `ureq` client
  bridged to an async SSE response (no new dependency) and the unchanged JSON
  envelope (so the candid, jargon-free persona is untouched); the model streams
  `stream: true` and the node incrementally extracts the growing reply. The
  non-streaming `POST /v1/chat` remains for the CLI and as a fallback. Your message
  is still persisted before the model is called, so an interrupted stream never
  loses the turn.
- **Built-in self-signed HTTPS** (`ENDORA_TLS=1`): the node can serve HTTPS with a
  self-signed certificate (persisted next to the database, so the browser warning is
  accepted once), making the web console a **secure context** — which browsers
  require for the microphone / voice input. No domain, CA, or reverse proxy needed;
  set `ENDORA_TLS_SAN` to the host's LAN IP/hostname for a cleaner cert. The console
  and [hosting guide](docs/hosting.md) explain enabling voice. (Fully local Whisper
  STT — audio never leaving the host — is the next voice step.)
- **The butler is grounded in your life (1.0 integration)**: each turn the butler
  is handed a `ButlerContext` — your values, your North Stars (with status, the
  value each serves, and whether it has a target yet), and what needs attention —
  so it speaks about what actually exists and proposes the next concrete step,
  including a **target under an existing North Star** (a new `create_target`
  proposal). Verified live: the butler referred to an existing North Star by name.
  (Cutting the `1.0.0` release remains a human decision.)
- **Voice & character (0.9)** ([ADR 0017](docs/adr/0017-persona-and-voice.md)):
  the butler's prompt now **mirrors the person's register** — matching warmth and
  formality — with the golden-rule floor (reflect kindness upward, never hostility
  downward), on top of stored style preferences; truthfulness is never softened.
  **Voice** is browser-native and opt-in: the console reads the butler's replies
  aloud (`speechSynthesis`) behind a Speak toggle and captures a spoken message
  (`SpeechRecognition`) from a 🎤 button — no backend change, no dependency, and it
  hides where the browser lacks the API. Local-first caveat stated: browser speech
  recognition may use a cloud service, so it stays opt-in.
- **Anti-sycophancy eval harness (completes 0.7)**
  ([ADR 0014](docs/adr/0014-the-butler-conversation-values-attention.md) §5): an
  opt-in eval (`endora-infrastructure` integration test, `#[ignore]`d so CI needs no
  model) baits the butler with prompts that tempt flattery/reflexive agreement and
  checks it stays candid. Run on demand against a live model
  (`ENDORA_MODEL_URL=… ENDORA_MODEL=… cargo test … --test butler_eval -- --ignored`).
  Sycophancy is treated as a defect measured by evals, not a code gate.
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

### Fixed

- **Chat could hang "forever" and lose its place on reload.** The butler's call
  to the local model had no timeout, so a slow/stuck model (e.g. inference on CPU)
  left the "thinking…" indicator spinning indefinitely, and reloading dropped the
  client-only indicator so you couldn't tell if a reply was still coming. Now the
  model round-trip is bounded (90s) and always falls back to the scripted reply, so
  a reply is always persisted; and the thinking indicator is **derived from stored
  state** (shown whenever your message is the newest one), so it survives reloads
  and reflects reality. Your message is still saved before the model is called.

### Changed

- **The butler talks like a person, not a schema** ([ADR 0017](docs/adr/0017-persona-and-voice.md)):
  the `Value → North Star → Target` structure is the butler's *internal model* of you
  and your browsable profile — not conversational vocabulary. The system prompt now
  forbids the taxonomy words ("value", "North Star", "target", "goal", …) in the
  butler's spoken reply and grounds it in plain language; confirm-card labels read as
  natural actions ("Keep this as something you're working toward: …") rather than
  schema ("Create North Star: …"). The structured overview keeps the evocative labels
  as the transparency window. A unit test guards the conversation against jargon.
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
