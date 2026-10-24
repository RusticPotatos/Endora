# Agent collaboration: dormant NAS deployment

2026-10-22. AI-assisted release record on `codex/agent-collaboration`.
The owner approved the reviewed packet and instructed deployment. Application
candidate `753984ee` was deployed from a clean detached checkout under `~/repos`.
The unrelated baseline-collector edit was preserved outside the release checkout.

## Deployment and safeguards

The established workflow was used: build on the configured separate Docker
machine for the NAS's `linux/amd64` architecture, then Endora's
`make deploy-check` to transfer images, drain HQ, replace services, lift the
drain and run live smoke checks. The build machine's Docker service was started
first because it was stopped. No compilation ran on the NAS.

The ansible-nas `make run-remote TAGS=endora` preview showed that its configured
Endora application block is skipped: this installation is operated by Endora's
Make deployment. Ansible manages the shutdown drain. The first preview also
selected an OpenBao task; it was excluded with `EXTRA=--skip-tags openbao`.
The narrowed preview was reviewed before apply. The apply passed and changed
only the shutdown-drain script, unit and enablement. Independent systemd checks
confirmed the unit is active and enabled. No ansible-nas source or private input
files were edited. The pinned collection was unchanged because this slice changes
no deployment role, Compose or Make configuration.

Consistent node and HQ database snapshots were made through the running image's
SQLite snapshot command. HQ's database path was explicitly supplied when invoking
that command inside its container. Snapshots were integrity-checked, retained on
the NAS and copied privately for restoration/upgrade rehearsal. The old application
image was retained under a release-specific tag. These are recovery assets, not
proof that an old binary may open the upgraded database.

Both restored database copies were opened twice using the candidate's actual
store adapters. The node adapter applied its schema with the vault locked;
the rehearsal neither decrypted secrets nor started an application, model or
worker. Both copies passed SQLite integrity checks and retained every preexisting
table's row count. Production secret recovery was not rehearsed. Node and HQ
mount sources were compared with the resolved deployment configuration and matched.

An explicit preliminary drain observed no running turns before the Ansible apply.
It was lifted during the subsequent build of enabled companion images. The Make
deployment's final drain again explicitly reported no running turns before any
application restart. No drain-failure fallback was used. The standard target also
built/transferred the enabled sound/AirPlay images; the AirPlay sender and AirPlay
2 services were recreated. The existing runner/worker source did not change, so
no Pi runtime deployment was needed.

## Observed results

- `make deploy-check` exited successfully. Both live smoke tests passed;
  one test was filtered out by the existing invocation.
- `/health` reports status `ok`, version `0.12.0`, build `753984ee`.
- Node and HQ run the exact application image built for this candidate; its
  full identity is recorded in the private release manifest.
- HQ's drain is lifted and reports no cut turns. The team's preexisting global
  stop state is preserved: normal team operation remains active.
- Eight members and nine open tasks remain visible. A post-deployment SQLite
  integrity check passed, and every pre-deployment HQ table's row count was
  retained or increased. These checks establish preservation at the aggregate
  level; they do not independently re-evaluate every historical task.
- Conversations remain disabled and the conversation directory is empty.
  No pilot participants, conversation or assistance request were created in
  production, and no pilot model call was made.
- The actual served sign-in screen visibly shows build `753984ee`. This browser
  has no owner session, so signed-in Work controls and physical devices remain
  unverified. The smoke checks do not substitute for that pilot/UI evidence.

Private evidence is retained in the ignored `.local/release-753984ee/` directory:
release manifest, prior and deployed container identities/configuration, snapshots,
schema-rehearsal results, scoped Ansible previews/apply, drain logs, deployment log
and live health/HQ summaries. Credentials, network addresses, database contents
and full raw logs are not included in this public record.

## Remaining boundary

This installs the dormant feature. It does not complete the
[live diagnostic pilot](agent-collaboration-live-pilot-2026-10-22.md), its matched
comparison, Phase 5 device evidence or Phase 6 adoption decision. Pilot activation
still requires its scoped authorization and preflight. Forward disable remains
the supported response to a collaboration failure; restoring older data requires
an explicit decision because it could discard later work. See the
[reviewed implementation plan](agent-collaboration-plan-reviewed-2026-10-21.md)
for the remaining gates.

## Replacement cutover: `6e67eb6c`, 2026-10-24

AI-assisted release of the [ADR 0138](../adr/0138-one-conversation-and-assistance-lifecycle.md)
replacement from a clean detached checkout of `6e67eb6c`. Its executable code is
unchanged from `25edbd06`, whose full gate passed (see the
[checkpoint](agent-collaboration-foundation-checkpoint-2026-10-22.md)). The live
build `753984ee` is an ancestor of `6e67eb6c`, and `develop` is fully merged, so
the release reverts no deployed work.

Before the cutover:

- Production snapshots of the node and HQ databases were opened twice by the
  candidate's store adapters and by a compatible-restore build, offline and
  without network, secrets, workers or model calls. Integrity, foreign keys,
  original columns and row counts were preserved; the backups were unchanged.
- The ansible-nas preview of the Endora block changed nothing; it manages only
  the shutdown drain, as in the previous release. The Pi was reachable and its
  runner untouched.
- The NAS (`amd64`) and Pi (`arm64`) images were built off the NAS. The first
  workbench builds failed while Ansible Galaxy returned HTTP 500 for every
  collection download; the unchanged build passed once Galaxy recovered.
- The running NAS node and the Pi's node/workbench images were tagged
  `before-replacement-6e67eb6c` as the rollback release. Fresh integrity-checked
  node and HQ snapshots were written to the NAS backups folder immediately
  before the restart; they are its rollback database pair.
- Production HQ held two legacy consultations, both answered: no queued or
  running legacy work needed draining or cancelling.

Cutover:

- `make deploy-check`: the drain reported 0 running turns, node, HQ and the
  sound senders/AirPlay 2 services were recreated, the drain was lifted with
  0 cut turns, and both live smoke tests passed (one filtered out, as before).
- `make deploy-pi`: the drain again reported 0 running turns; the Pi's runner
  restarted on the new node and workbench images and lifted the drain with 0 cut.

Observed results:

- `/health` reports status `ok`, version `0.12.0`, build `6e67eb6c`; the Pi
  runner reports build `6e67eb6c`.
- HQ before/after snapshots both pass integrity and foreign-key checks. No table
  was dropped and no table lost rows; the five new tables are the replacement's
  history and provenance tables. Only the event and game logs grew in between.
- The old consultation engine is not in this build; its tests show v1 worker
  calls receive HTTP 410. The startup cancellation had nothing pending.
- The first real turn after the restart ran in the new workbench on the Pi and
  exited 0. Neither the runner nor HQ logged an error.
- Information messaging was already enabled before this release, and still is.
  Production has no authorized conversation, so new peer help fails closed until
  the owner authorizes one. Signed-in Work UI and physical device checks were
  not repeated against production in this release.

Rollback is the tagged prior images with the pre-restart snapshot pair, restored
as one compatible release/database pair; it would discard work recorded after
the restart, so it needs an explicit decision. Private evidence (logs, snapshots,
container identities and before/after comparisons) is in the ignored
`.local/release-6e67eb6c/` and `.local/replacement-release-readiness-6e67eb6c/`
directories.

## Authorize form fix: `161e27da`, and the first conversations, 2026-10-24

The owner's first attempt to authorize a conversation was refused with "Choose
two different members." HQ redraws every few seconds; the form's two member
pickers started on the same member and kept no pick across a redraw. `161e27da`
keeps the picks and the open form across redraws and starts the second picker
on a different member. A console check failed on the old form and passes on the
fix; the full repository gate passed (2,355 tests, console, personal-value,
collection parity and whitespace checks).

Released the same way: the running image was tagged `before-161e27da`, fresh
node and HQ snapshots were integrity-checked, and `make deploy-check` drained
with 0 running turns, cut none and passed both smoke tests. The served console
shows build `161e27da`. Only the console changed, so the Pi was not redeployed.

The owner then authorized, in Work, three conversations on Home lab upkeep:
Bo with Cy, Ada with Bo and Dee with Kit. All three are empty and are kept as
set up. New peer help between those pairs can now be admitted, charged to that
project, within the existing per-turn and per-day limits.

Not yet observed in production: real work triggering a peer-help request, its
answer and the requester's retained finding. That depends on the team's own
work needing a colleague; it is the remaining production evidence for ADR 0138.
Signed-in Work was exercised for the conversation list, authorize form and an
empty history page. Private evidence is in the ignored `.local/release-161e27da/`
directory.

Phone check and `c19da567`: at a 375 px phone viewport the conversation list,
a history page and its assistance page rendered without horizontal overflow,
but their controls were 31 px tall. `c19da567` gives them the 44 px style the
task history already uses; a console check failed before and passes after, and
the full gate passed again (2,355 tests). Released with a `before-c19da567`
image tag, fresh integrity-checked snapshots and `make deploy-check` (0 running
turns, 0 cut, both smoke tests passed). At phone width all 15 controls now
measure at least 44 px, nothing overflows, and the three conversations are
intact. This was the in-app browser's phone emulation, not a physical phone.
