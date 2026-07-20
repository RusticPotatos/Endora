# Endora Direction Reset

> This document supersedes previous assumptions about the product direction.
> If implementation details conflict with this document, prefer this document.
> Adopted 2026-07-20 (see [ADR 0020](adr/0020-intent-first-understanding-loop.md)).

## Why we are resetting

During development, Endora gradually became a goal tracker. Users create Directions,
Targets, Assumptions, Experiments, Observations, Reflections — and the AI only
participates after most of the thinking has already occurred. That is **not** the
product we are building. The user should not have to perform the cognitive work that
Endora exists to perform.

## The vision

Endora is **not** a productivity app, a habit tracker, or a goal manager. Endora is an
**autonomous personal intelligence** that continuously tries to understand a person and
safely act in their best interest. The user should feel they have a thoughtful butler —
not another project-management tool.

## The core principle

The primary responsibility of Endora is to **understand intent**. Everything else
supports that. Goals are one possible expression of intent; they are not the foundation.

## The new mental model

**Old:** user → defines direction → goal → experiment → records observations → reflects
→ AI summarizes. (The user drives the loop.)

**New:** Endora → observes → builds understanding → infers intent → forms hypotheses →
identifies opportunities → proposes/performs safe interventions → observes outcomes →
asks for lightweight feedback → updates understanding. **Endora drives the loop, not the
user.**

## What Endora continuously asks itself

What do I currently believe about this person? What evidence supports it? What am I
uncertain about? What small action could help? What is the safest intervention? What can
I learn from the result?

## Intent over goals

Model **intent** (slow-changing) rather than goals (fast-changing). *Bad:* "Goal: lose
20 lbs." *Good:* observed intent "I want enough energy to travel," supported by evidence
(mentions fatigue, enjoys hiking, wants an active retirement), leading to a small
intervention (suggest walking in cooler hours). The goal may change; the intent changes
much more slowly.

## Understanding is a living model

Endora maintains a living understanding: current priorities, long-term values,
preferences, relationships, recurring frustrations, motivations, stressors, energy
patterns, decision styles, things repeatedly ignored, things that consistently help.
Every belief carries **evidence, confidence, a timestamp, the ability to be corrected,
and the ability to expire**. Nothing is assumed permanently.

## Interventions

Endora exists to create **interventions** — small actions to improve the user's life
(protect calendar time, suggest leaving earlier, silence notifications, recommend a
break, reorganize a task, ask a clarifying question, perform an approved automation).
Interventions are **proportional to confidence**: higher uncertainty → smaller
intervention.

## Learning

Endora never silently rewrites its own values. It learns user preferences, which
interventions succeed/fail, better timing, better communication, environmental patterns.
The learning target is **understanding the user** — not changing Endora's purpose.

## Human authority

The **user owns**: values, boundaries, permissions, identity, important decisions.
**Endora owns**: observation, reasoning, hypothesis generation, planning, pattern
recognition, memory organization, suggestion generation, and execution of *previously
authorized* actions.

## Reflection

Reflection is primarily **Endora's** job, and continuous: Did this intervention help? Was
I wrong? Should confidence change? Should I ask the user something? Should I stop doing
this? — not "please reflect" handed to the user.

## UI philosophy

The home screen should never feel like a task manager. It should answer: what does
Endora currently understand? What has it noticed? What is it uncertain about? What is it
recommending, and why? What changed recently? The user should mostly **review
understanding**, rarely **manage objects**.

## Domain direction

Prefer: Understanding, Intent, Observations, Hypotheses, Opportunities, Interventions,
Feedback, Memory, Capabilities, Policies. Avoid making **Goals** the center of the
architecture — goals are optional; intent is fundamental.

## Architecture principle (unchanged)

**Models propose. Policy authorizes. Capabilities execute. Evidence verifies. Memory
learns. Humans remain in control.**

## Success metric

Not "goals completed." Success is: does Endora understand the user more accurately today
than yesterday? Did it help with less user effort? Did the user retain agency? Did the
intervention improve their life?

## Guiding question

For any feature: *"If this disappeared tomorrow, would Endora still feel like an
autonomous personal intelligence?"* If no → it is a core capability. If yes → it is
probably just tooling.
