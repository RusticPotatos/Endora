# Agent-to-agent protocol for Endora

Status: research proposal, 2026-10-21. Prepared with AI assistance. This is not an
accepted ADR or an implementation plan approved for deployment.

## Recommendation

Pilot **durable, directly addressed agent messaging with a shared inspectable
record**, hosted by Endora's existing runtime. Keep MCP for tools. Replace
communication that depends on task threads and reporting lines with authenticated
agent interactions. Make the office a view of those interactions and their results.

The Linux Foundation's **Agent2Agent (A2A) protocol** is the strongest candidate
reviewed for interoperability with other agent runtimes. Do not make a wholesale
internal A2A conversion a prerequisite for improving Endora's collaboration.
The deployed-system evidence below supports runtime-native messaging first and
an A2A adapter when an actual interoperability use case needs it.

This is an architectural recommendation inferred from the sources below, not a
claim that installing an SDK removes Endora's workflow rules. A2A standardizes
interoperation; Endora still decides who runs, what wakes them, what they may do,
and how much work is useful.

## What deployed systems and Anthropic use

Evidence checked 2026-10-21. "Anthropic's recent report" could refer to several
publications; they answer different questions and should not be treated as one
recommendation.

| System or publication | Documented communication | What it establishes |
| --- | --- | --- |
| Claude Code agent teams | Named agents use `SendMessage`; each recipient has a JSON-file mailbox. There is also a shared task list. Messages arrive automatically, and each teammate keeps its own context. Teams remain experimental and disabled by default. [Documentation](https://code.claude.com/docs/en/agent-teams#architecture) | An actual runtime-native messaging mechanism, not evidence that Claude Code uses A2A. |
| Anthropic Research, 2025-06-13 | A lead delegates to subagents and synthesizes their findings. At publication subagent batches ran synchronously; asynchronous steering was described as future work. The article recommends storing outputs as artifacts and passing references to avoid repeated summarization. [Engineering account](https://www.anthropic.com/research/multi-agent-research-system) | Tool-mediated delegation and artifact references work without a peer-network protocol. |
| LangChain | Documents subagents called as tools and handoffs that change the active agent using state. [Patterns](https://docs.langchain.com/oss/python/langchain/multi-agent) | Collaboration patterns are application/framework behavior, not inherently an external wire standard. |
| AutoGen Core | Documents typed messages sent directly to a recipient or published through topics and subscriptions. [Messaging](https://microsoft.github.io/autogen/stable/user-guide/core-user-guide/framework/message-and-communication.html) | An actor-style runtime can support direct interaction and publish/subscribe without using a task-thread board as its coordination model. |

The **2026 Agentic Coding Trends Report**, page 8 (PDF page index 7), forecasts
orchestrators coordinating specialized agents with separate context windows and
synthesizing their results. It emphasizes decomposition, coordination and human
oversight. It does not select an agent wire protocol in the examined report.
This is a trends report, not a specification or comparison of A2A and mailboxes.
[Official report](https://resources.anthropic.com/hubfs/2026%20Agentic%20Coding%20Trends%20Report.pdf)

The **2026-08-13 research article, "Patterns and problems in emerging multiagent
systems,"** describes experimental swarms using a shared forum, individual VMs
and, for software projects, a shared repository. Prescriptive roles and a CEO
hierarchy prompt made little difference in those game-building experiments;
that does not show hierarchy is never useful for other tasks or models. Its
conclusion warns that transmitting context costs agents about as much as acting
on it, so assumptions behind human meetings and organizations do not necessarily
transfer. It treats reliable peer coordination as an open interaction-design
problem; this is not an endorsement of a particular messaging standard.
[Research and conclusion](https://www.anthropic.com/research/multiagent-systems)

The newer **"Measurements for understanding the pace of AI development inside
frontier labs"** provides especially relevant implementation evidence. In its
Appendix, "Oversight of agents," Anthropic describes stable agent identities
independent of the underlying model, and shared open messaging with identity-linked
messages, original references and transcript cross-links. Agents can see one
another's messages, check claims and coordinate; monitors can trace interactions.
This is a description of its internal scaffold, not an instruction to expose all
Endora conversations or private data publicly. Its account does not identify an
A2A wire implementation.
[Internal platform account](https://www.anthropic.com/institute/measuring-pace-of-ai-development)

None of these examined Anthropic documents recommends adopting A2A as the solution
to internal coordination. That is a bounded finding about these sources, not a
claim about every Anthropic publication or private system. The stronger lesson
for Endora is to remove compulsory administrative steps while preserving identity,
original evidence, bounded context, reliable delivery and observable collaboration.
Text is a useful content format; mandatory task bureaucracy is the problem.

## Options checked

| Option | Current evidence | Fit for Endora |
| --- | --- | --- |
| A2A | Latest tagged specification release is v1.0.1, published 2026-05-28. The release changelog is dated 2026-05-26. [Release](https://github.com/a2aproject/A2A/releases/tag/v1.0.1) | Best external interoperability candidate reviewed, with multiple official SDKs including Rust. [SDK list](https://a2a-protocol.org/latest/sdk/) |
| IBM/BeeAI Agent Communication Protocol (ACP) | Its own documentation says ACP joined A2A. Its repository was archived on 2025-08-27 and links to migration guidance. [Project](https://github.com/i-am-bee/acp), [documentation](https://agentcommunicationprotocol.dev/introduction/welcome) | Do not introduce the retired standalone protocol. |
| Agent Network Protocol (ANP) | v1.2, released 2026-09-24, covers decentralized identity, naming, discovery and messaging. The release explicitly distinguishes published specifications from implementation support; negotiation remains draft and group E2EE remains candidate. [Release](https://github.com/agent-network-protocol/AgentNetworkProtocol/releases/tag/v1.2) | Worth revisiting for federation across independent owners. Its identity and cryptographic messaging machinery solve a broader problem than this local team needs. |
| Agent Client Protocol, also called ACP | A separate protocol for editors/IDEs communicating with coding agents. [Introduction](https://agentclientprotocol.com/get-started/introduction) | Could serve a future coding-runtime adapter; it is not the peer collaboration contract proposed here. |
| MCP | Connects agents to tools and resources; A2A's documentation describes the two as complementary. [Comparison](https://a2a-protocol.org/v1.0.1/topics/a2a-and-mcp/) | Retain for the chess tool, repository tools, diagnostics and the capability bridge. |

ANP has Rust support too: its first-party SDK repository lists a published Rust
crate and describes identity, proof, naming and selected E2EE support. Lack of
Rust is not a reason to reject it. The mismatch is scope, and published SDK
support does not establish full v1.2 messaging conformance.
[SDK evidence](https://github.com/agent-network-protocol/anp#sdks-and-releases)

## What A2A changes

A2A permits a direct message response for a simple exchange, or a stateful Task
for work that continues asynchronously. A context groups related exchanges.
That Task is a protocol execution record, not a mandate for a manager, subtasks,
claims or approval ceremonies. Terminal tasks are not reopened; follow-up work
can create another task in the same context.
[Interaction lifecycle](https://a2a-protocol.org/v1.0.1/topics/life-of-a-task/)

Agent Cards describe capabilities and interfaces. Cards can be supplied through
private configuration rather than a public registry. Start with Endora's known
members and advertised capabilities; public discovery adds nothing to this pilot.
[Discovery](https://a2a-protocol.org/v1.0.1/topics/agent-discovery/)

A2A supports text and structured content, output artifacts, status events,
JSON-RPC, HTTP+JSON and gRPC. Wire version `1.0` excludes the specification patch
number. Its specification explicitly warns that message history can be incomplete
and disconnected streams can miss updates. It is not a reliable delivery queue.
[Specification sections 3.6, 3.7, 4 and 5](https://a2a-protocol.org/v1.0.1/specification/)

Natural language remains useful for explanations and questions. Structured
payloads carry positions, references, actions and results. Models should not
parse a paragraph to discover whose move it is.

Authentication is established by the transport, with advertised security schemes;
credential acquisition and application authorization remain implementation work.
For Endora, derive the sending member from its authenticated turn, never from a
claimed name in message text. Keep the gateway and deterministic capability checks.
[Security model](https://a2a-protocol.org/v1.0.1/topics/enterprise-ready/)

## Apply it to the existing architecture

[ADR 0102](../adr/0102-endora-hq.md) makes work a task tree and restricts speech
to task threads. The current [worker tools](../../domains/team/src/application/protocol.rs)
expose `post`, `create_task`, `set_status` and escalation through `ask_owner`.
Changing only their wire encoding would preserve the bureaucracy.

The [worker](../../app/worker/src/main.rs) takes one turn in a disposable
container and exits. [HQ authenticates its live turn token](../../app/hq/src/team.rs),
and the [runner](../../app/runner/src/main.rs) starts containers for scheduled
turns. Give each member a stable logical address hosted by existing HQ and
fronted by the node, with an A2A endpoint when interoperability requires it.
Persist incoming work before scheduling a worker. An agent
identity need not mean an always-running process or a new service.

| Replace for the pilot | Retain |
| --- | --- |
| Task-thread membership as permission to speak | Authenticated identity, activity access and capability checks |
| Mentions in prose as the delivery address | Explicit recipient, correlation and delivery records |
| Mandatory manager relay for ordinary questions | Coordinator help when the activity needs it |
| Task creation and status changes merely to take a chess turn | Existing runner, isolated workers and metered model calls |
| Office animation inferred from administrative events | Honest views of recorded interactions, actions and artifacts |

Keep real-work commitments and independent verification where they serve the
activity. A protocol completion report must not by itself pass Endora's evidence
gate. The game engine, not a player's completion claim, decides a chess result.
[ADR 0135](../adr/0135-scenarios-worlds-the-team-acts-in.md) already establishes
that world-owned result and records the failures caused by scenario tasks being
treated as stuck office work.

Direct addressing means logical peers within Endora's policy boundary. It does
not require arbitrary connections between worker containers. Each agent can act
as client and server through its adapter. A shared room and multi-party routing
are application behavior, not a group-chat guarantee supplied by A2A. Map the
Endora activity to the contexts returned by each endpoint instead of assuming
that one identifier automatically shares every participant's memory.

## Smallest useful pilot

Use two agents and one authoritative chess match. Start with Endora's own
addressed-message contract and existing transport; add no broker or federation
service. Expose communication through a small worker tool adapter. If the pilot
also tests a second A2A runtime, select HTTP+JSON first and keep that adapter
separate from the activity's rules.

HQ sends the next player a structured board position and revision. The player
acts through the existing checked chess tool. A successful move commits the
world change and durable notification of the next player together; a retry must
not apply the move twice. Optional conversation is directly addressed. The
match's structured result and replay become an artifact the office can open.
This payload and delivery behavior are an Endora contract, whether transported
internally or through A2A.

The reliability layer needs a durable inbox/outbox, authenticated sender and
recipient, deduplication, acknowledgements, bounded retries, deadlines and
cancellation. Enforce board revisions as well as message deduplication: two
different requests can still propose the same stale move. Separate information
from requests that require an answer, and cap wake cycles, turns and spend.
The [display feed](../../app/hq/src/main.rs) prunes old events, so it must not
be used as the delivery queue.

Accept the pilot only when these three checks hold:

1. A match reaches its engine-decided result without task claims, management
   escalation, reviews or manual status reports. The office shows its real moves,
   refusals, participants and final result.
2. Restarting HQ or a worker, duplicating a request, and disconnecting a viewer
   neither lose an accepted move nor apply it twice. Reconnection rebuilds the
   view from persisted authoritative state.
3. An expired token, forged sender, stale board revision, exhausted budget or
   cancelled match cannot advance play or start an unbounded exchange.

Chess proves transport, state and recovery. Before replacing real-work
communication, also run one diagnostic collaboration: one agent asks another a
question, receives evidence and returns an independently checked result. Measure
useful actions, stalled exchanges, turns, cost and human interventions against
the existing design.

## Version and readiness limits

For an A2A adapter, use the released v1.0.1 schema/tag as the reference and request
wire version `1.0`. The versioned documentation still contains a banner naming
v1.0.0 as latest; the release page is better evidence of the tagged patch release.
Do not copy old 0.3 examples or track `/latest` silently. Record schema and SDK
versions together and test the selected binding against another implementation.
[Version rules and migration appendix](https://a2a-protocol.org/v1.0.1/specification/),
[release](https://github.com/a2aproject/A2A/releases/tag/v1.0.1)

The official [Rust SDK](https://github.com/a2aproject/a2a-rs) implements client
and server libraries, HTTP+JSON, JSON-RPC, gRPC and SSE. Its README requires Rust
1.85 or newer. Its component crates currently have separate pre-1.0 versions;
the SDK version is not the protocol version. Pin compatible published crates
after a dependency review rather than assuming any crate named `a2a` is this SDK.
[Workspace manifest](https://github.com/a2aproject/a2a-rs/blob/main/Cargo.toml)

No SDK was built or interoperability-tested during this research. Endora's exact
dependency compatibility, recovery behavior and cross-SDK conformance remain
unverified. Neither a stable protocol release nor an official SDK listing proves
the proposed integration works. Before implementation, record the accepted
architecture and its changes to ADR 0102/0135 in a new ADR; this note authorizes
no deployment.
