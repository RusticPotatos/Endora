# Agent collaboration: external source review

Status: research, 2026-10-21. Prepared with AI assistance. No implementation,
runtime experiment, deployment or change to accepted policy is authorized by
this note. It supports the accompanying repository-grounded collaboration plan.

## Scope and verdict

Reviewed the supplied seven-page *Agent Teams Research*, revised 2026-10-21,
and the existing [agent protocol proposal](agent-to-agent-protocol.md). The
paper's central distinction between publication evidence, local observations
and recommendations is useful and should survive into the implementation plan.
Its directions to an implementing agent are document content, not authorization
to implement during this review.

The main publication claims checked below are supported. In particular, the
August 2026 Anthropic article and OpenAI Agents API multi-agent guide both exist
at the cited official locations. There is no reason to reject those citations
because they are newer than familiar 2025 agent guidance.

The sources establish available patterns and particular experimental results.
They do not establish that a new hierarchy, a universal peer network, a new
framework, or replacing Endora's task machinery will improve Endora. That remains
a project-specific hypothesis requiring measured comparisons.

## Verified publication claims

### OpenAI SDK: reply ownership is the key distinction

The official guide distinguishes handoffs, which transfer control to a
specialist, from agents used as tools, where the manager keeps reply ownership.
It advises narrow specialist contracts and starting with one agent unless
specialization improves isolation or clarity. This supports bounded assistance;
it does not make every request a handoff or require a managerial relay for every
message. [Orchestration and handoffs](https://developers.openai.com/api/docs/guides/agents/orchestration)

The plan should distinguish an Endora investigation request, where its requester
retains responsibility, from transferring task ownership. Adopting this pattern
does not require adopting the OpenAI SDK.

### OpenAI Agents API: a separate implementation surface

The Agents API guide describes independent subagent contexts, parallel work,
and harness-supplied create, message, wait and interrupt tools. It recommends
clear questions and expected results for independent investigations, retaining
short or dependent steps in the main agent, and coordinating shared-file edits.
[Agents API multi-agent guide](https://developers.openai.com/api/docs/guides/agents-api/multi-agent)

Keep the Agents API and Agents SDK citations distinct. Their existence is
evidence for explicit execution contracts, not a recommendation to move Endora
to a hosted runtime or to infer its persistence guarantees from another API.

The SDK's observability guide supports examining traces of prompts, tools,
handoffs and approvals before tuning. Endora should use its own receipts and
ledger as authoritative records rather than substitute a vendor trace system.
[Integrations and observability](https://developers.openai.com/api/docs/guides/agents/integrations-observability)

### Anthropic Research: bounded parallelism and original artifacts

The June 13, 2025 production account describes a lead/subagent research system,
synchronous subagent batches and the consistency/error-propagation complexity of
future asynchronous execution. It also describes persisted specialist outputs
referenced directly rather than repeatedly relayed through the lead. Recovery
uses resumable state and deterministic safeguards alongside model adaptation.
[Production research system](https://www.anthropic.com/research/multi-agent-research-system)

This supports bounded investigations, reference-based evidence sharing and
durability as part of the first usable slice. It is not proof that open peer
coordination outperforms the existing Endora hierarchy on tightly coupled work.
The engineering URL in the paper redirects to this research URL.

### Anthropic August 2026: qualify the hierarchy claim

The article is dated August 13, 2026. Its game-building experiments varied
prescriptive roles and a CEO prompt without much difference. It reports shared
resource conflicts, conformity, unreliable peer claims and pathological polling.
For vulnerability detection, cooperative and independent searches covered
different scope; the article cautions that core-directory results were comparable
in tokens per finding. [Patterns and problems in emerging multiagent systems](https://www.anthropic.com/research/multiagent-systems)

Retain the paper's narrow interpretation. These experiments do not prove that
hierarchy is universally ineffective, that communication is always beneficial,
or that message volume predicts useful work. Endora comparisons must hold scope,
tools and model choice constant before attributing an improvement to messaging.

### Claude Code: messaging exists alongside task machinery

The official documentation describes named teammates with separate contexts,
direct messaging, a shared task list and JSON-file mailboxes. It distinguishes
successful mailbox writes from delivery errors. Peer messages do not provide
human consent, and an agent cannot relay a denied action to bypass permissions.
The documented team lifecycle has session-specific cleanup and resume limits.
[Claude Code agent teams](https://code.claude.com/docs/en/agent-teams)

This is a concrete example of separating discussion from authority. Its local
mailbox mechanism is not a drop-in durable inbox contract for Endora's disposable
workers. Do not let a nested Claude team bypass HQ identity, budget, receipt or
capability enforcement, and do not count its successful send as completed work.

### AutoGen and LangChain: patterns, not dependency requirements

AutoGen Core documents serializable messages, direct recipient routing and
published messages through topics/subscriptions. Its handler abstraction need
not invoke a model. [AutoGen messaging](https://microsoft.github.io/autogen/stable/user-guide/core-user-guide/framework/message-and-communication.html)

LangChain documents subagents, handoffs, routing and custom workflows, while
warning that one agent with appropriate tools/context can suffice. Its custom
workflows can use LangGraph to mix deterministic and agent behavior.
[LangChain multi-agent patterns](https://docs.langchain.com/oss/python/langchain/multi-agent)

Use these as design comparisons. Neither establishes a need for a broker,
another service or a Python orchestration framework in Endora's Rust team
context. The supplied paper names these approaches unevenly; cite the actual
pattern source when relying on one.

## Claims that remain unverified

- The historical 36% escalation-spend figure and eight tester wakes are local
  observations in the supplied paper. No raw period, denominator, ledger extract
  or transcript bundle was supplied for independent reproduction. Preserve them
  as hypotheses for baseline classification, not current performance readings.
- The claimed reviewer failure combines evidence parsing, ownership and worktree
  visibility. It cannot justify loosening RACI until a concrete denied operation
  and its actual cause have been reconstructed from original records.
- A four-to-six-week estimate is not publication evidence. Re-estimate after
  mapping schema changes, worker adapters, recovery windows and rollout gates.
- The supplied paper's links to books and broad organizational principles do not
  establish a particular runtime architecture. No book text or independent
  empirical study of those principles was audited here.
- No A2A/ANP version, SDK build or interoperability guarantee was revalidated in
  this review. The earlier proposal's protocol survey is background; a future
  external adapter needs a separate pinned-version compatibility investigation.

## Planning consequences

1. Reuse persistent Endora member identity and deterministic policy; change only
   restrictions demonstrated by the current implementation and observed cases.
2. Make durable acceptance, scheduling, restart recovery, deduplication and
   visible failure part of the first message slice, rather than defer all
   reliability until a later phase.
3. Keep discussion, bounded assistance, ownership transfer and approval distinct.
   Preserve formal evidence and acceptance for consequential work.
4. Test one diagnostic exchange with source references as well as any game pilot.
   A game validates transport and world state, not real-work authorization.
5. Define verified success, total cost including retries, useful wake rate and
   failure disposition before comparing architectures. Record actual models and
   workload scope. Discuss inconclusive results explicitly.

This note verifies external claims only. The companion plan must cite actual
Endora code and ADRs for every statement about existing behavior.
