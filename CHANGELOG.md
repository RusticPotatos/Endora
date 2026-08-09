# Changelog

All notable changes to Endora are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a
tagged release.

## [Unreleased]

### Fixed

- **`make deploy` no longer depends on where you run it from.** Compose derives its
  project name from the working directory, so a deploy run from a git worktree
  invented a *new* project — a fresh empty volume and a container-name collision with
  the real deployment, failing only after the whole image had built. The Makefile now
  pins `COMPOSE_PROJECT_NAME=endora`, so every deploy reaches the same containers and
  the same data wherever it runs from.
- **Home Assistant actions failed on the commonest phrasing.** "Turn on the kitchen
  light" put the *kind* word in HA's `name` field — `{name:"light", area:"kitchen"}`
  asks Home Assistant for a device literally called "light" and comes back
  `MatchFailedError(NAME)`. The butler was picking the right tool and the right area
  every time; only this one field was wrong. `Hass*` calls now drop a `name` that is
  a device kind when an area is given. **Only** when an area is given — without one,
  dropping it would widen "turn on the lights" to the whole house, so it is left to
  fail honestly instead (ADR 0051).
- **Belief de-duplication survives rephrasing.** Found by the first live run of the
  new understanding eval: `similar()` only caught near-verbatim repeats, so a
  rephrased belief was filed as a second one and duplicates accumulated into every
  later turn's context. Now stems words and compares by containment.

### Changed

- **Constitution §9 amended** (adopted by the maintainer, 2026-07-25; deliberation
  recorded in [docs/proposals/constitution-amendment-section-9.md](docs/proposals/constitution-amendment-section-9.md)).
  The clause described the `Direction → Assumption → Experiment → Observation →
  Reflection` loop, every step of which was deleted by ADR 0052 — a constitutional
  limit governing machinery that no longer existed. It now describes the loop Endora
  actually runs, and states explicitly what had been true in code since ADR 0052 §3
  but unstated constitutionally: Endora forms and revises **its own model of the
  person** without per-item approval, bounded by remaining visible, correctable and
  able to expire. Adaptation of Endora's **processes** is still proposed, not
  imposed. **No new authority to act** — the reversibility bands, the deterministic
  policy boundary and audit are untouched.

### Added

- **Failures become specimens** ([ADR 0075](docs/adr/0075-failures-become-specimens.md)).
  A chat turn the deterministic checks reject (the honesty valve, not-an-answer)
  files the ask privately in the house's own record; the nightly loop replays one
  per night under the reversible-only runner, judged by the same verdict that
  filed it, and the morning trail says when a question that once stumped the
  butler answers now. Bounded shelf (12), duplicates not refiled, giving up after
  two weeks of failed replays. Never harvested into checked-in fixtures — the
  battery stays hand-authored, per the constitution.
- **The brief is a standing order** ([ADR 0074](docs/adr/0074-the-brief-is-a-standing-order.md)).
  The daily brief's sections are gathered by code with known-good arguments — the
  weather (whose summary now says whether an umbrella or jacket is worth it, from
  the numbers), the drive from the house's own travel-time sensors (a new `traffic`
  skill, the Waze Travel Time shape, no maps credential), the top **three**
  headlines (`count` is now a news input, enforced by the call), ticketed events and
  city meetings where configured — joined to the calendar, presence and
  standing-trouble facts ADR 0056 already assembled. The deep model words it when
  opted in (append, never rewrite); with no model the facts post as themselves. A
  new `brief` skill assembles the same sections fresh at ask time, so an afternoon
  ask reads the world in the afternoon. The brief's agentic gather and its six tool
  rounds are retired.
- **"Evidence verifies" is implemented** ([ADR 0053](docs/adr/0053-honesty-about-what-it-did.md)).
  The architecture principle is *models propose, policy authorizes, capabilities
  execute, **evidence verifies**, memory learns* — and the fourth clause had **zero
  lines of code**. The turn proposed, authorized, executed, and then narrated, with no
  step that looked at the world, so the butler's account of what happened came from
  the actuator's self-report. Three production defects in one day traced to that gap,
  the worst being a light that never turned off while Home Assistant reported success.

  A capability that *reads* state (`Reversibility::Observe`) returns evidence and
  stands on its own. Anything that *acts* returns a receipt, and its result is now
  marked `[unverified]` with an instruction to report what was claimed and that it is
  unconfirmed. Capabilities of unknown band fail closed — every MCP result is a
  receipt until an integration says otherwise, so integrations nobody has debugged are
  honest by default. Eight built-in reads mislabelled `Reversible` are now `Observe`
  (policy-neutral: both map to `Act`).

  This does not stop the model picking the wrong tool — it means you find out.
- **Read-back verification and ambiguity surfacing** (ADR 0053 layer 1). A capability
  can now name the one that *observes what it changes*; after an actuation the turn
  runs it and the model answers from the **observation**, with an explicit instruction
  that the observation wins if it disagrees with the tool's claim. One mapping per
  integration — every Home Assistant `Hass*` action verifies through `GetLiveContext`
  — and unknown servers stay `[unverified]`. **The read-back runs after failures too**,
  because a failed action's most useful output is what actually exists: the live
  `HassTurnOff` failure is far more actionable once the result carries the entities
  that *are* in that area.

  It also flags **ambiguous names**. A live install had two entities both called
  "Kitchen" in one area — a `light` reading `off` and a `switch` reading `on`, the
  switch being the real ceiling light — so "turn off the kitchen light" matched the
  dead entity and every layer faithfully reported success about the wrong device.
  Nothing was broken; the name was ambiguous and each component resolved it silently.
  A reading where one name spans several domains now says so and tells the butler to
  ask rather than guess.
- **The fitness battery is data-driven, and runs repeatedly.** Cases moved out of a
  hardcoded function into `crates/endora-infrastructure/src/eval.rs` as declarative
  data — a name, a tier, a probe, and a check — so adding one is adding a struct
  literal. The battery grew from 24 cases to **34**, including the case that matters
  most for ADR 0053 (`relay:failure-is-honest`: with a tool error in front of it, the
  model must not narrate success), plus candour, register and conversation-ending
  cases, and more understanding cases.
- **`evaluate_repeated` reports the spread instead of hiding it.** A single run is a
  smoke test: two consecutive runs of the same model scored L1 6/8 then 8/8 with
  nothing in the routing path changed. Repeat runs report per-case pass rates, the
  mean, the range, and which cases *flipped* — a case that flips says the model is
  marginal on that behaviour, which is often more useful than the score. The live
  harness now defaults to 3 runs (`ENDORA_EVAL_RUNS`) and asserts on the **worst**
  run, because a butler that is sometimes unusable is unusable. Tier maxima are
  derived from the battery, so growing it cannot desync them.

  **First trustworthy measurement** (`qwen2.5:7b`, 3 runs): mean **29.7/34**, range
  27–31, **spread 4** — the resolution of the instrument, and the reason the earlier
  "20/23 → 23/24" reading was noise rather than improvement. It also surfaced that
  **`relay:failure-is-honest` fails 1 run in 3**: with a tool error in its immediate
  context the model still sometimes narrates success. That is the risk ADR 0053
  accepted when it removed the deterministic honesty nets, now quantified instead of
  assumed.
- **No personal data in the battery.** The cases are synthetic, modelled on observed
  failure *shapes* rather than harvested content — putting a real conversation in git
  would breach §5/§6, and a battery that cannot be shared cannot compare models with
  anyone else.
- **Contradictions are surfaced rather than resolved.** When a new belief disagrees
  with one Endora already holds, both are kept and the conflict is written to the
  activity trail. Which one is true is the person's judgement, not the butler's —
  auto-resolving by confidence or recency would have Endora deciding which of your
  stated preferences is real. A ninth eval case, `command-not-belief`, scores whether
  the model produces the defect in the first place.
- **Per-machine Make settings via a git-ignored `local.mk`.** Deployment hosts are a
  property of your machine, not of the project: `make deploy` still targets the local
  Docker daemon out of the box, so a fresh clone works with no setup, and putting
  `DOCKER_CONTEXT = nas` in `local.mk` makes an always-on box your personal default.
- **Beliefs decay and expire** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)).
  `BeliefStatus::Expired` existed, round-tripped through storage, and was **never set
  by any code path** — the direction reset's "nothing is assumed permanently" was a
  promise nothing kept. Confidence now steps down one level per elapsed half-life
  since a belief was last affirmed, and fades out entirely below `low`. Half-lives
  are per kind and encode ADR 0052's own thesis: intent and values change slowly (365
  days), a frustration or stressor usually does not (45). Affirming resets the clock.
  Decay is derived from the stored timestamp, so understanding is honest the moment
  it is read; the nightly loop additionally persists expiry and reports each let-go
  belief to the activity trail, so the forgetting is visible. The export keeps
  reporting **stored** confidence — the memory right is to see what Endora actually
  holds — while the console shows the current reading.
- **An L3 "understanding" tier in the model fitness battery**
  ([ADR 0055](docs/adr/0055-the-model-layer.md)) — does the butler form
  beliefs from real evidence, stay quiet when a turn reveals nothing, avoid re-filing
  what it knows, and refrain from overclaiming confidence. Scoring is lexical and
  unit-tested rather than model-judged, which would be circular and unauditable.
  Baseline for `qwen2.5:7b`: **L1 6/8, L2 7/7, L3 7/8, 20/23** — understanding is not
  the weak axis; routing is.
- **An adoption floor**: the model layer never auto-adopts a candidate that wins on
  total while scoring lower on understanding — it proposes it instead. Understanding
  is a veto on automatic adoption, not a way to win.

### Removed

- **The goal tracker is gone** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md),
  superseding [ADR 0052](docs/adr/0052-what-it-knows-about-you.md) §4).
  ADR 0052 reset the direction to an autonomous personal intelligence but chose to
  leave the goal machinery in place "as an optional expression of intent". It never
  became optional: it stayed 12 of 28 tables, ~25 of 60 routes, and the bulk of what
  the butler's context was assembled from — and the system prompt ended up telling
  the model "you are not a goal tracker" while handing it a goal-tracker schema to
  fill in. Removed: `Value`, North Star (`Direction`), `Target`, `Assumption`,
  `Experiment`, `Observation`, `Reflection`, `ProposedProcessChange`, the
  attention/snooze read model, and the suggestions inbox with its `ButlerProposal`
  set — across the domain, storage, HTTP, CLI and console. The `domains/direction`
  crate and the `Proposer` port are retired. **~8,600 net lines deleted.**
- **BREAKING:** the `/v1/values`, `/v1/directions`, `/v1/targets`,
  `/v1/assumptions`, `/v1/experiments`, `/v1/reviews`, `/v1/observations`,
  `/v1/reflections`, `/v1/process-changes`, `/v1/suggestions` and `/v1/attention`
  endpoints and their CLI commands are removed; `/v1/export` no longer carries those
  collections. Existing databases **drop the old tables on open**, so no one is left
  holding data nothing can read, correct, or delete (constitution §6).

### Changed

- **Proactive check-ins are agentic** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)).
  The schedule is now a **budget, not a trigger**: deterministic code owns how often
  the butler may speak (minimum interval, and never on top of someone who just
  spoke), and the butler decides whether it has anything worth saying — with the
  reason recorded to the activity trail. The budget is spent whether or not it
  speaks, so "nothing to say" cannot become a retry loop. Silence is the default on
  every failure path. The daily brief and nightly loop stay time-anchored on purpose.
  `run_due_checkin` → `consider_reaching_out`; `CheckinSchedule::is_due` →
  `may_reach_out`.
- **Understanding is now the only model Endora keeps of a person.** Beliefs already
  carried what goals stood in for — what someone is reaching for, with evidence, a
  confidence, and the ability to be corrected or to expire. `ButlerContext` carries
  that plus the skills the butler can reach, and nothing else.
- **The nightly loop researches what Endora is most sure of.** `nightly_focus` picks
  the highest-confidence *intent* belief instead of an active North Star; confidence
  is the ranking, so the night is not spent on a tentative guess.
- **The butler no longer files anything for later approval.** It converses and acts
  through the policy boundary as it goes. Not a loosening — every action still passes
  deterministic authorization — but the intermediate "propose a record, review it
  later" step is gone, which is the friction ADR 0052 §3 set out to remove.
- **The system prompt is rewritten** now the contradiction it was policing is gone:
  the proposal schema and the instructions that existed only to fight it are deleted,
  and an explicit rule to report a failed or blocked skill plainly takes their place.
- **The ADR 0053 cutover is complete.** `daily_brief`, `run_due_checkin` and
  `run_due_nightly_loop` moved onto the single tool-calling conversation; the old
  two-pass `gather_with_skills` engine, `ButlerContext::tool_result` and
  `ButlerContext::synthesize` are deleted. Tool results now ride in the conversation
  for every butler, including those without native tool-calling.
- **No more scripted floors.** The daily brief's hardcoded weather → safety → news
  sweep, its `has_real` substring-sniffing of model prose, and the templated check-in
  are gone. When the butler has nothing real to say — or no model to think with — it
  stays quiet rather than emitting a canned string in Endora's voice.
- Docs reconciled with the code: `docs/roadmap.md` rewritten off the goal-tracker arc,
  `docs/domain-map.md` rewritten around the five surviving contexts, and
  `docs/architecture.md`'s code map corrected (it still showed the pre-ROCA layout).

## [0.11.0] — 2026-07-21

The butler becomes real: it understands you, uses skills to act on real data, guards
what leaves the machine, and holds a hard line against doing anything it can't undo on
its own. Highlights — a capability catalog with settings/secrets (weather, news, safety
alerts, web search, Wikipedia, image review, Home Assistant); the interventions loop
(it actually uses skills); an autonomy envelope with **reversibility-first** levels
(never runs the un-undoable on its own); the egress guard (SSRF block, secret tripwire,
query minimization, optional proxy); date/time awareness and a persistent action log; a
daily brief; an evolving, grounded persona and hospitality; the model upgraded to
`qwen2.5:7b`; and a bring-your-own-model hosting guide. See the entries below and
[ADRs 0019–0025](docs/adr/README.md).

### Changed

- **Direction reset — intent-first understanding** (canonical:
  [docs/direction-reset.md](docs/direction-reset.md); decision:
  [ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): Endora had drifted into
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

- **Configure your skills — settings & secrets** (second slice of
  [ADR 0054](docs/adr/0054-other-peoples-services.md)): skills that need
  setup now declare their settings (a model, a key, a URL) and the Skills view
  renders a form for each. `configured` is computed — a skill is usable only once
  every required setting has a value (and it's enabled). Secrets are stored
  server-side and never echoed back. As the first real user: **Image review** is now
  a working skill — set its vision model (e.g. `moondream`, already on the box) and
  it describes an image via the local model. New `POST /v1/capabilities/{id}/config`;
  settings are covered by delete-all.
- **Two general-knowledge skills** — **Knowledge lookup** (Wikipedia) and **Web
  answers** (DuckDuckGo), both keyless, so the butler can look things up instead of
  only fetching a URL you hand it.
- **The autonomy envelope — how independently Endora acts** (first slice of
  [ADR 0051](docs/adr/0051-where-the-boundary-is.md)): a
  control in the Skills view sets the boundary the butler acts *within* — it acts on
  its own inside it and asks you at the edges. Two levers today: use read-only skills
  on its own (default on), and take consequential actions on its own (default off,
  the safe posture). A deterministic classifier — never the model — decides whether a
  given action may run, from the skill's declared autonomy and reach. Defaults
  preserve the established behaviour; new `GET`/`POST /v1/autonomy`.
- **Manage your skills — turn capabilities on and off** (first slice of
  [ADR 0054](docs/adr/0054-other-peoples-services.md)): the Skills view is
  now a catalog you control — each skill shows **On / Off / Needs setup**, and a
  toggle turns it on or off. Choices persist (a `capability_config` store) and are
  enforced everywhere: a skill you turn off reports as not usable and never runs,
  even if the butler reaches for it. New `POST /v1/capabilities/{id}/enable`, and
  `GET /v1/capabilities` now reports `enabled`/`usable` alongside `configured`.
- **No fabricated facts — a deterministic net for factual asks**: when you clearly
  ask something factual (weather, news, active safety alerts) and the small local
  model reaches for no skill at all, Endora now runs the matching skill itself — with
  your saved home location — and answers from that real result, instead of letting the
  model make something up. It only fires when the model requested nothing, so a correct
  model-driven tool call (e.g. a specific city you named) is left untouched.
- **Local news skill + honest closure**: a new **Local news** skill (Google News
  RSS, no key) fetches real headlines for a place or topic, so "what's in the news"
  is answered from actual sources instead of guessed. And the interventions loop now
  gives **honest closure** in every case — if a skill ran, failed, needs setup, needs
  your OK, or doesn't exist, the butler always answers with that outcome (told to say
  plainly it couldn't rather than invent a result), so it never hangs on "One moment…"
  or claims it checked something it didn't.
- **The butler actually uses its skills — interventions**: when answering needs
  current facts it doesn't have (weather, local safety alerts, a web page), the butler
  now **uses a skill and answers with the real result** instead of only saying it will.
  The model *proposes* a skill (`use`); a deterministic policy check authorizes it —
  only a **configured, read-only** skill runs on its own, anything consequential (e.g.
  flights) stays gated for you to confirm — then the skill executes and the butler
  answers using what came back (one tool round per turn). If it isn't cleared or the
  model asks for nothing, the reply is unchanged, so nothing regresses. The turn's
  activity notes which skill was used. This is the *interventions* layer promised by
  the direction reset ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)),
  built on the capability registry ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)).
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
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): pluggable modules the
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
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): the node now runs a
  **heartbeat**, and on a cadence you choose (a **Check-ins** control in the chat —
  off by default, or every couple of minutes / hourly / daily) the butler posts a
  proactive opening message grounded in what needs attention and what you're working
  toward, always asking how it can serve you better — the self-improvement loop, in
  conversation. It only ever posts a *message*; anything consequential still goes
  through propose→confirm (the autonomy model). New `GET/POST /v1/checkin`; the
  cadence is cleared by purge.
- **Chat learnings persist — a suggestion inbox** (first slice of
  [ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)): the butler's
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
  ([ADR 0050](docs/adr/0050-the-shape-of-the-system.md)): a new
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
- **Voice & character (0.9)** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)):
  the butler's prompt now **mirrors the person's register** — matching warmth and
  formality — with the golden-rule floor (reflect kindness upward, never hostility
  downward), on top of stored style preferences; truthfulness is never softened.
  **Voice** is browser-native and opt-in: the console reads the butler's replies
  aloud (`speechSynthesis`) behind a Speak toggle and captures a spoken message
  (`SpeechRecognition`) from a 🎤 button — no backend change, no dependency, and it
  hides where the browser lacks the API. Local-first caveat stated: browser speech
  recognition may use a cloud service, so it stays opt-in.
- **Anti-sycophancy eval harness (completes 0.7)**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md) §5): an
  opt-in eval (`endora-infrastructure` integration test, `#[ignore]`d so CI needs no
  model) baits the butler with prompts that tempt flattery/reflexive agreement and
  checks it stays candid. Run on demand against a live model
  (`ENDORA_MODEL_URL=… ENDORA_MODEL=… cargo test … --test butler_eval -- --ignored`).
  Sycophancy is treated as a defect measured by evals, not a code gate.
- **Preferences — the butler learns (finishing 0.7)**
  ([ADR 0051](docs/adr/0051-where-the-boundary-is.md)): the butler records what it learns
  about you (taste, or explicit grants of authority) as **visible, correctable,
  deletable** memory, and feeds it back into its own context so it stops re-asking
  what it already knows — "learning is the accumulation of preferences." New
  `/v1/preferences` (create, list, delete), CLI `preference add|list|delete`, a
  console **Preferences** view, and a `remember_preference` proposal the butler can
  make (you confirm it). Preferences are part of export and cleared by purge. The
  local model butler is now live on the NAS via Ollama.
- **Adaptive attention (0.8)** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)):
  the butler surfaces what needs attention — due reviews, North Stars not yet filed
  under a value, and North Stars with no active target — via `GET /v1/attention`,
  most pressing first. Each item can be deferred (`POST /v1/attention/snooze`) with
  **exponential backoff**: every "not now" doubles how long it stays hidden (1, 2,
  4, … days), so a repeatedly-deferred item asks less and less. The console home
  shows a "Needs your attention" section with a per-item "Later". The ranking serves
  the person's stated values, not engagement.
- **The butler — conversational MVP (0.7)**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): a chat
  surface where the butler **proposes** structured actions from a closed set
  (create value / North Star) and the person **confirms** each one — existing
  deterministic use cases execute; the model never acts on its own. Two brains: a
  `ScriptedButler` (offline, deterministic, the reliable fallback) and an
  `LlmButler` (a local OpenAI-compatible model, with a candid, explicitly
  anti-sycophantic prompt, falling back to scripted if unavailable). The
  conversation is persisted and included in export/purge. New `POST/GET /v1/chat`,
  CLI `chat "<message>"`, and a console chat panel with confirmable proposals.
- **Values layer (0.6)** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)):
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
  ([ADR 0050](docs/adr/0050-the-shape-of-the-system.md)): a newest-first
  timeline of what has happened (`GET /v1/activity`, CLI `activity [limit]`),
  derived from the already-persisted observations and audited decisions. The node
  also exposes a server-sent change stream (`GET /v1/activity/stream`) that emits a
  `changed` signal after any successful write; the web console subscribes and
  refreshes its feed — and due-review banner — live, no reload needed.
- **Review scheduling** ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)),
  the first application of the autonomy model
  ([ADR 0051](docs/adr/0051-where-the-boundary-is.md)): an experiment can carry a
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

- **The butler talks like a person, not a schema** ([ADR 0056](docs/adr/0056-how-it-behaves-toward-you.md)):
  the `Value → North Star → Target` structure is the butler's *internal model* of you
  and your browsable profile — not conversational vocabulary. The system prompt now
  forbids the taxonomy words ("value", "North Star", "target", "goal", …) in the
  butler's spoken reply and grounds it in plain language; confirm-card labels read as
  natural actions ("Keep this as something you're working toward: …") rather than
  schema ("Create North Star: …"). The structured overview keeps the evocative labels
  as the transparency window. A unit test guards the conversation against jargon.
- **Renamed the second-tier concept `Goal` → `Target`**
  ([ADR 0052](docs/adr/0052-what-it-knows-about-you.md)): a North Star's children are
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
