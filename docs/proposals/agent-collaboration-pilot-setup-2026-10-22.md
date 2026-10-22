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
