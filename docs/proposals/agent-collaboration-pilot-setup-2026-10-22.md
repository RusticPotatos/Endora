# Agent collaboration: configured isolated pilot

2026-10-22. Prepared with AI assistance. The owner requested completion of the
agreed pilot setup. This record distinguishes configured resources from a live
model result; no diagnostic model call has occurred.

## Actual configured resources

The ignored isolated pilot store now holds:

- `Member 1`: supporting lead, paused; no model turn.
- `Member 2`: requester, local work brain and standard worker runtime.
- `Member 3`: expert, local work brain and standard worker runtime.
- A synthetic goal and active project, with an empty external-tool allowlist.
- One owner-authorized conversation with exactly the requester and expert.
- One 24-hour grant for the original receipt of reading the synthetic report.

The numeric ID mapping and grant expiry are in the private `allocated-ids.json`
packet. All previously seeded members are paused; every routine, including those
added on service startup, is disabled. Conversation permission is enabled in this
isolated store, while the team's global stop is engaged and no run remains live.
The actual services were stopped after verification. The NAS deployment and its
conversation switch were not changed during this setup.

An operator used the same public owner application use cases invoked by the
owner HTTP handlers to create the resources. A real RemoteRunner setup turn was
allocated to the requester, without starting a worker or model. The operator
read `diagnostic-report.txt` and recorded that successful read through the
application's live-token receipt use case. The source is attributed to operator
setup. The setup turn ended successfully, and its temporary task was dropped;
no task was accepted as an agent diagnosis or repair. No raw SQL was used to
insert artifacts, identities, receipts or classifications.

## Corrections found during setup

The original packet named a general document-creation tool that this project
does not expose. Documents belong to branch submissions and check results.
The operational fixture is therefore now `diagnostic-pilot-v2-receipt`, using
the supported original receipt source type. The report text, expected facts,
read-only objective and independent requester inspection requirement remain
the same. The frozen B1-B9 baseline was not changed retrospectively.

The project domain also rejects a zero cap. Its bookkeeping cap is unset;
that is not a paid-call allowance. Paid use remains prohibited by the zero-dollar
experiment scope, local-only dispatch requirement, empty new node provider store
and per-process network fence. The future live supervisor must refuse cloud
brains before dispatch, regardless of a member's judgment-brain default.

## Verification actually performed

The pinned actual HQ and node binaries were started briefly on dedicated
loopback ports, using only the isolated databases and fresh pilot credentials.
The authenticated node proxy returned the configured conversation, exact pair
and project, one active original-source grant and an empty request page.
HQ's snapshot confirmed global stop, all nonparticipants paused, all routines
disabled and zero running turns. The node's owner API also switched all external
contracts and container restarts off. Both processes were terminated afterward.

The native application build is the previously pinned candidate; application
source is unchanged between it and the deployed application candidate. Binary
and model identities remain in the private manifest.

macOS per-process sandbox profiles were prepared for HQ, node, worker and model:
the worker may reach only pilot HQ/node listeners; the node may reach only pilot
HQ/model listeners; HQ and the model have no outbound destination permission.
The services successfully bound their selected loopback listeners under those
profiles. Actual connection checks established permitted paths and denied a
nonpilot local listener, direct worker-to-model access and an external destination.
A permissive-profile negative control reached the nonpilot listener and was
rejected by the intended predicate. One extra profile check initially conflicted
with a listener used by service verification; it was rerun sequentially after
service shutdown and passed. No host firewall or NAS network was changed.

Private evidence lives in `.local/agent-collaboration-pilot/`: allocated IDs,
revised fixture, actual report, setup helper, service verification helper/result,
network profiles/results, bounded dispatcher, private service logs and updated
manifest. It contains
no copied production database or production credential.

## Execution boundary

Configured resources are complete; the live diagnostic remains unrun. Existing
request policy supplies its 15-minute deadline, six expert read rounds and three
attempts per association. The private dispatcher now enforces an exclusive
process lock for one worker, eight total physical starts, one initial requester
start, at most three expert and three result starts, and a 20-minute session
watchdog. Every worker starts with the pinned standard worker's six-model-round
limit; its loop makes one model request per round, bounding eight starts to at
most 48 model calls. Failed starts consume the persisted allowance before
spawning. The dispatcher refuses cloud brains, foreign participants, task-owned
turns, shell/workbench runtimes, unlinked result/expert runs and nonempty
external-contract listings.

Without inference, its admission preflight accepted the intended synthetic
shape and rejected altered brain, identity, shell, task, start count, repeated
requester, deadline and unarmed-state inputs. The actual command entry point
also refused an unarmed session and an already-held worker lock, before touching
the network or starting a worker. The execution envelope remains unarmed with
zero starts. These verify launch guards, not a model result: model loading and
the complete live worker/relay exchange remain unexercised. Do not substitute an
unrestricted runner at activation.

The [pilot packet](agent-collaboration-live-pilot-2026-10-22.md) remains the run
contract. This setup is not Phase 4's live pass, its matched comparison, Phase 5
device verification or Phase 6 adoption. The retained grant must be checked at
activation and, if expired, renewed only for the same original source and pair.

## Foundation candidate refresh

The isolated packet now pins application commit `6ff6341a`, including the
integrated ordinary consultation path and subsequent lifecycle/accounting fixes.
The three actual native binaries were rebuilt with the locked build command in
the pilot packet. The build passed. Previous binaries, manifests and consistent
offline SQLite backups of both stopped pilot stores were retained privately
before replacing the pilot copies. Production services and databases were not
accessed or changed by this refresh.

Actual HQ and node services reopened the existing synthetic records under their
network profiles. The authenticated proxy still reports the fixed project/pair,
one original-source grant and zero requests. The same receipt grant was renewed
through the owner API for 24 hours; no source or participant was added. The node
now has one local-only provider and enabled Everyday entry, with a newly generated
pilot-only model key. Provider discovery and inference were not invoked. The
requester's frozen initial objective and independent result-reading instructions
are owner-authored notebook notes. An initial oversized note was refused by the
existing 300-character limit; two valid notes were then saved without changing
that limit. No wake or request was created.

Review found that the original dispatch supervisor bounded physical starts but
did not itself enforce the packet's one-logical-request cap within a requester
turn. A private experiment protocol fence now mediates worker calls to the real
HQ protocol. It reserves the first exact frozen `consult` intent before
forwarding it; only identical retries may follow, including after a fence restart.
The application's own idempotency, live-token checks and authorization still
decide whether HQ accepts it. The experiment fence cannot grant access.

The worker network profile permits only this fence and the isolated node;
direct worker-to-HQ and worker-to-model connections are denied. The fence accepts
only the current worker token and diagnostic-stage calls: its briefing, the
fixed conversation, the granted original, the initial frozen consultation and
the linked expert answer. Other tools, cancellation, changed questions, another
request origin, unrelated sources and recursive consultation are refused before
forwarding. Tool listings are passed through unchanged, so a refusal is visible
if the model proposes an out-of-scope operation. This is a constrained research
harness, not a new application service or proof of unrestricted production use.

Seven private guard tests passed, including an actual HTTP upstream test of
exact retries, pre-forward refusal, foreign credentials, retained intent across
fence restart and the unarmed state. A permissive authorization stub failed 15
assertions before the guard was implemented. The connection matrix was rerun
against actual listeners with the revised worker profile; allowed paths worked,
forbidden paths failed, and a permissive-profile negative control reached the
nonpilot listener as expected. All application, model and runtime fingerprints
matched. Dispatch admission preflight and actual unarmed-entry refusal passed.
These tests do not claim a completed live worker exchange.

A private activation helper was built against this candidate's application
crate. Its preflight validated the stopped store and same granted source without
creating a wake. Its actual wake entry point refused an unarmed envelope before
resuming the team or creating work. The helper binary and source are pinned in
the private manifest. The pinned runtime's help also confirms support for the
isolated model-key file; no model was loaded to inspect that option.

Final offline checks found a healthy SQLite store, zero live runs, zero assistance
requests, no nonzero ledger rows, zero physical worker starts and an unarmed
envelope. All pilot listeners were closed after verification. The manifest pins
the supervisor and profile fingerprints as well as the application binaries.
Private credentials, machine paths and raw records remain ignored. The existing
unrelated baseline-collector edit was preserved and was not used as a checker.

The complete application repository gate was rerun and passed: formatting,
workspace Clippy with denied warnings, workspace all-feature tests, console
checks, personal-value checks, collection parity and whitespace checks. The team
suite passed 923 tests and its behavior harness passed 162 scenarios and 2,055
steps. Existing explicitly ignored tests remained ignored. This refresh changed
only ignored experiment helpers/configuration and these public records; it did
not change application behavior. No terminal-client source changed, and no new
physical-device or deployment validation was performed.

Current verdict: prepared and stopped; live outcome cannot-tell. The next
activation is one synthetic diagnostic on this refreshed candidate, using the
fixed model/settings, zero paid calls, at most eight worker starts, six model
rounds per start and a 20-minute watchdog. The initial worker must use ordinary
`consult`; the expert and result workers must read the same original. Model
loading, complete worker/relay exchange, source-read reconciliation, latency and
result evaluation remain unrun. Explicit live authorization is still required
by the agreed packet; preparing it has not armed it. Matched comparison, device
checks and production release remain separate gates.
