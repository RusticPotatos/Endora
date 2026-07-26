# Archive

The decisions Endora was actually built from, in the order they were made.

They were consolidated on 2026-07-26 into [seven records](../README.md): forty-nine
documents had become a history you had to read to find a rule, and the rules are the point.
Nothing here was reversed by that consolidation — a decision superseded on its merits says
so in its own Status line.

**Read these when you want to know *why*.** A consolidated record states what the system
does; these state what it was like before, what was tried, and what it cost to find out.
That reasoning is what stops a rule being argued away later by someone who only has the
rule.

| # | Decision | Consolidated into |
| --- | --- | --- |
| [0001](0001-modular-monolith.md) | Domain-first modular monolith | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0002](0002-rust-authoritative-core.md) | Rust for the authoritative core | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0003](0003-http-json-openapi-protocol.md) | HTTP + JSON + OpenAPI application protocol | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0004](0004-sqlite-first.md) | SQLite-first persistence | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0005](0005-models-propose-policy-authorizes.md) | Models propose; policy authorizes | [0051 Where the boundary is](../0051-where-the-boundary-is.md) |
| [0006](0006-first-vertical-slice.md) | First vertical slice: the learning loop for one goal | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0007](0007-async-web-stack.md) | Async runtime and web stack for the node | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0008](0008-local-model-adapter.md) | Local model adapter | [0055 The model layer](../0055-the-model-layer.md) |
| [0009](0009-node-served-ui-and-single-container.md) | Node-served web UI and single-container packaging | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0010](0010-autonomy-model.md) | Autonomy model: the act/ask loop and preferences | [0051 Where the boundary is](../0051-where-the-boundary-is.md) |
| [0011](0011-review-scheduling-reminders.md) | Review scheduling: the first act of the autonomy model | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0012](0012-activity-feed-and-change-stream.md) | Activity feed and a server-sent change stream | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0013](0013-rename-goal-to-target.md) | Rename the second-tier concept from Goal to Target | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0014](0014-the-butler-conversation-values-attention.md) | The butler: conversational interface, the Values layer, and adaptive attention | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0015](0015-identity-and-values-context.md) | The Identity & Values context: a "why" above North Stars | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0016](0016-adaptive-attention.md) | Adaptive attention: ranking and deferral-backoff | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0017](0017-persona-and-voice.md) | Persona and voice | [0056 How it behaves toward you](../0056-how-it-behaves-toward-you.md) |
| [0018](0018-streaming-chat-responses.md) | Streaming chat responses | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0019](0019-proactive-self-improving-butler.md) | The proactive, self-improving butler (heartbeat, check-ins, capabilities, hospitality) | [0056 How it behaves toward you](../0056-how-it-behaves-toward-you.md) |
| [0020](0020-intent-first-understanding-loop.md) | Intent-first: the autonomous understanding loop (direction reset) | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0021](0021-capability-catalog-and-mcp-host.md) | The capability catalog: configuration, enablement, and an MCP host | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0022](0022-autonomy-envelope-and-self-authored-capabilities.md) | The autonomy envelope and self-authored capabilities | [0051 Where the boundary is](../0051-where-the-boundary-is.md) |
| [0023](0023-egress-guard-and-data-loss-tripwire.md) | The egress guard: SSRF protection and a data-loss tripwire | [0051 Where the boundary is](../0051-where-the-boundary-is.md) |
| [0024](0024-reversibility-aware-autonomy-and-the-nightly-loop.md) | Reversibility-aware autonomy and the nightly self-improvement loop | [0051 Where the boundary is](../0051-where-the-boundary-is.md) |
| [0025](0025-hospitality-and-the-evolving-persona.md) | Hospitality and the evolving persona | [0056 How it behaves toward you](../0056-how-it-behaves-toward-you.md) |
| [0026](0026-package-by-bounded-context.md) | Responsibility-Oriented Clean Architecture (app / domains / shared) | [0050 The shape of the system](../0050-the-shape-of-the-system.md) |
| [0027](0027-self-improving-model-layer.md) | The self-improving model layer (model discovery, eval, gated adoption) | [0055 The model layer](../0055-the-model-layer.md) |
| [0028](0028-native-tool-calling-turn.md) | One native tool-calling turn (grounded honesty, no deterministic narration) | [0053 Honesty about what it did](../0053-honesty-about-what-it-did.md) |
| [0029](0029-delete-the-goal-tracker.md) | Delete the goal tracker; understanding is the only model | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0030](0030-measuring-understanding.md) | Measuring understanding: the L3 eval tier and the adoption floor | [0055 The model layer](../0055-the-model-layer.md) |
| [0031](0031-agentic-proactivity.md) | Agentic proactivity: a budget, not a trigger | [0056 How it behaves toward you](../0056-how-it-behaves-toward-you.md) |
| [0032](0032-beliefs-decay-and-expire.md) | Beliefs decay and expire | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0033](0033-what-understanding-admits.md) | What understanding admits: instructions out, contradictions kept apart | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0034](0034-evidence-verifies.md) | Evidence verifies: an unobserved effect is never reported as fact | [0053 Honesty about what it did](../0053-honesty-about-what-it-did.md) |
| [0035](0035-outcomes-what-happened-after-acting.md) | Outcomes: what happened after Endora acted | [0053 Honesty about what it did](../0053-honesty-about-what-it-did.md) |
| [0036](0036-durable-intentions.md) | Durable intentions: work that outlives a turn | [0052 What it knows about you](../0052-what-it-knows-about-you.md) |
| [0037](0037-disclosure-not-persuasion.md) | Disclosure, not persuasion: an unverified action is always visible | [0053 Honesty about what it did](../0053-honesty-about-what-it-did.md) |
| [0038](0038-capability-profiles.md) | Capability profiles: learning what a tool does, instead of patching each one | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0039](0039-capability-repair-proposals.md) | Capability repair proposals: noticing its own tooling is wrong | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0040](0040-withdrawing-a-capability-that-never-works.md) | Withdrawing a capability that never works | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0041](0041-searching-the-reading-for-the-real-target.md) | Finding the thing the person meant | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0042](0042-direct-reach-into-a-service.md) | Direct reach into a service | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0043](0043-writing-names-back-into-the-service.md) | Writing names back into the service | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0044](0044-policy-acts-on-what-it-has-established.md) | Policy acts on what it has established | [0051 Where the boundary is](../0051-where-the-boundary-is.md) |
| [0045](0045-an-undo-log-for-what-it-changed.md) | An undo log for what it changed | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0046](0046-a-made-up-category-is-part-of-the-target.md) | A made-up category is part of the target | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0047](0047-a-thing-and-its-diagnostics-are-not-two-things.md) | A thing and its diagnostics are not two things | [0054 Other people's services](../0054-other-peoples-services.md) |
| [0048](0048-the-tightest-match-wins.md) | The tightest match wins | [0054 Other people's services](../0054-other-peoples-services.md) |
