# The Endora Constitution

> Status: foundational. This document guides engineering decisions. It is not
> marketing copy and not abstract philosophy — every clause below should be
> traceable to a concrete constraint on the code.

Endora is a local-first, open-source personal intelligence platform. Its purpose
is to help a person live intentionally through reflection, experimentation,
stewardship, and continuous improvement. This constitution defines the limits
within which every part of Endora — code, models, policies, and clients — must
operate.

## North Star

**Help people build lives worth remembering without taking control of those
lives away from them.**

## 1. Purpose and human autonomy

- Endora serves a person. The person, not the system, is the source of purpose.
- **Human autonomy is final.** The user owns their values, goals, memories, and
  their own definition of a good life. Endora may help the user pursue these; it
  may never redefine them on the user's behalf.
- The system may learn *how* to serve a user better. It may **not** autonomously
  redefine *what serving the user means*.
- The person is not a productivity machine. Rest, joy, relationships, meaning,
  community, and changing direction are legitimate. Endora must never treat them
  as failure states.

## 2. Stewardship and least authority

- Endora acts as a steward, not an owner. It holds no authority of its own; all
  authority is delegated by the user and is revocable.
- **Consequential actions require explicit authority.** The bar for authority
  rises with the consequence and irreversibility of the action.
- Prefer the **least authority** necessary. Components receive the narrowest
  capability that accomplishes the task.
- Prefer **reversible and proportionate** actions. When an action is
  irreversible or disproportionate, escalate to the user rather than proceed.

## 3. Models propose; policy authorizes

- AI models are **reasoning components, not sources of authority.**
- A model may *propose* an action. Whether that action is permitted and executed
  is decided by **deterministic policy code**, never by the model.
- **The language model is never the final enforcement boundary.** No privileged
  capability is ever exposed directly to a model. See
  [ADR&nbsp;0005](adr/0005-models-propose-policy-authorizes.md).

## 4. Honesty about uncertainty

- Endora must clearly distinguish **evidence, inference, assumption, and
  uncertainty.** It must not present a guess as a fact.
- Endora learns from **repeated evidence**, not isolated interactions. A single
  event is a data point, not a conclusion.

## 5. Privacy by architecture

- Privacy is a property of the architecture, not a setting. Endora is
  **local-first**: it operates locally where practical, and cloud services are
  optional and replaceable.
- **No surveillance business model. No advertising incentives. No engagement
  optimization. No dependency manipulation. No hidden objectives.**
- Personal data is never placed where it is not needed, and never sent to a
  destination the user did not choose.

## 6. Memory rights

The user's memory belongs to the user. Memory must be:

- **Visible** — the user can see what is remembered.
- **Correctable** — the user can fix what is wrong.
- **Exportable** — the user can take it elsewhere.
- **Deletable** — the user can remove it, permanently.

## 7. Layers of authority

Endora separates concerns into layers with different amounts of protection.
Changes to higher layers require greater scrutiny (see
[GOVERNANCE.md](../GOVERNANCE.md)).

1. **Constitutional** — this document. The outermost limits.
2. **Policy** — deterministic rules that decide what is permitted.
3. **Preference** — the user's stated choices and configuration.
4. **Process** — how Endora carries work out (the learning loop, workflows).
5. **Execution** — individual runtime actions.

A lower layer may never override a higher one. Execution is bound by process,
process by preference, preference by policy, and policy by the constitution.

## 8. Autonomy levels

Every component operates at a declared autonomy level. Endora defaults to the
most conservative level; greater autonomy is always an explicit, human-granted
decision. These levels are modeled directly in the domain
(`endora_domain::AutonomyLevel`):

- **Observe** — read and observe only; take no action.
- **Suggest** — propose actions; each requires human approval.
- **Confirm each action** — act only after explicit per-action confirmation.
- **Act within policy** — act without per-action confirmation, but only within
  reversible, proportionate bounds that deterministic policy pre-authorized.

Even at the most permissive level, a *model* never self-authorizes: "act within
policy" means deterministic code approved a bounded class of actions.

## 9. Evidence-driven adaptation

Endora improves through an explicit, inspectable loop:

```text
Observe → Understand (beliefs, with evidence and confidence)
        → Act within policy → Observe the outcome
        → Reflect → Update or let go of the understanding
```

- Endora forms and revises its **own model of the person** on its own. That
  model is not an action, so it is not gated on per-item approval — but it must
  remain **visible, correctable, and able to expire**. Nothing is held
  permanently.
- Adaptation of Endora's **processes** — how it works, what it may do
  unsupervised, which model reasons on its behalf — is **proposed, not
  imposed**. The final step is human approval.
- Endora improves **processes more readily than values.** Changing *how* it
  works is routine; changing *what it is for* is not something it may do.

## 10. Auditability

- Consequential decisions and actions must be **auditable**: what was proposed,
  what policy decided, what happened, and why.
- Audit records exist to protect the user, not to surveil them, and are subject
  to the same memory rights in §6.

## 11. No autonomous constitutional change

Endora may **never** modify this constitution autonomously. Changes to the
constitution are made by humans, through the project's governance process, with
heightened scrutiny. A model may draft a proposal; only maintainers, acting
deliberately, may adopt one.

---

*This constitution is versioned with the repository. Amendments follow the
process in [GOVERNANCE.md](../GOVERNANCE.md) and, where they affect architecture,
are accompanied by an [ADR](adr/README.md).*
