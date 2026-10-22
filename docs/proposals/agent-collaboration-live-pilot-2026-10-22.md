# Agent collaboration: isolated live pilot packet

2026-10-22. Prepared with AI assistance on `codex/agent-collaboration`.
The owner authorized preparing this pilot. Live model execution is awaiting
approval under [ADR 0137](../adr/0137-addressed-conversations-and-bounded-assistance.md).
This packet narrows the next experiment; it does not authorize production access,
paid calls, deployment or broader adoption.

## Candidate and preparation evidence

Application candidate: `21dfca63` and its ancestors, including Phase 3
`fe4571bc`. Subsequent packet documentation does not change application behavior.
The unrelated working edit to `scripts/agent-collaboration-baseline.py` was not
included in the build or changed by this preparation. Do not run that edited
baseline collector as the pilot's acceptance checker.

The three actual binaries (`endora-hq`, `endora-node`, `endora-worker`) were built
from this worktree with `cargo build --locked -p endora-hq -p endora-node -p
endora-worker`. The build passed. Cargo reported the existing ignored terminal
package profile; the workspace profile applies. Copies and SHA-256 fingerprints
are in the ignored `.local/agent-collaboration-pilot/` packet, together with:

- `manifest.json`: candidate, binary/model fingerprints, runtime version,
  proposed settings, limits, preparation observations and empty approval field.
- `fixture.json`: frozen synthetic report, objective and expected facts.
- `hq.db`: fresh preparation store, globally stopped with conversations disabled.
- `hq-token`: newly generated owner credential, mode 0600, for this store only.
- `bootstrap.log` and `bootstrap-snapshot.json`: isolated startup evidence.
- `evidence-template.json`: unmeasured results, initially `cannot-tell`.

No production database or credential was copied. HQ briefly ran on a dynamically
allocated loopback port with no runner credential. Two seeded runs were stopped;
no worker was dispatched. The HQ process was terminated after persisting stop-all
and feature-off. No node, model server or worker was started. Tokens, private
paths and raw snapshots stay ignored; sanitize any later published evidence.

The full application gate is recorded in the
[hardening checkpoint](agent-collaboration-hardening-checkpoint-2026-10-22.md).
This preparation rebuild is additional evidence, not another full suite run.

## Deployment ownership and the existing NAS workflow

Owner clarification: NAS deployment must follow the established ansible-nas
workflow and shared standards. The local bootstrap above was preparation evidence;
it does not establish that the candidate has been installed or verified on the NAS.

The existing workflow has two cooperating parts:

- Endora's [Makefile](../../Makefile) builds the image on the configured separate
  build machine and transfers it to the target. `make deploy-check` also applies
  Compose, waits for the node and runs the live smoke checks. It changes running
  services and belongs inside the approved release window.
- ansible-nas's `nas.yml` invokes `endora.homelab.endora` with tag `endora`.
  Its `requirements.yml` pins the collection from this repository. The
  [Endora role](../../deploy/ansible/collection/roles/endora/tasks/main.yml)
  manages the checkout, host settings and Compose services, with `build: never`
  and `pull: never`. Its [guide](../../deploy/ansible/collection/roles/endora/README.md)
  requires matching settings with Endora's deploy configuration.

For an approved NAS release, keep these steps in one recorded release window:

1. Select a clean committed application candidate and record image IDs/digests,
   the collection revision and `endora_version`. The collection currently pinned
   in the inspected ansible-nas checkout is `a0806e31`; that is a checkout
   observation, not proof of the NAS's installed version. Check whether a
   collection update is needed and review its diff before changing the pin.
   Do not let a moving source ref or a later Ansible run revert the candidate's
   Compose configuration. Review any ansible-nas changes on that repository's
   own branch under its AGENTS.md; this preparation changes no files there.
2. Confirm private targets and configuration from the approved local settings.
   Keep images, Compose project name, node/HQ data locations, enabled profiles
   and model settings consistent between the Make deployment and the Ansible
   role. Do not read or modify ansible-nas inventories, group/host variables or
   vault files during this review. An operator resolves private inputs at release.
3. Complete consistent node and HQ backups and the restore/upgrade rehearsal
   required by the reviewed plan. Retain prior image identities and compatibility
   evidence. The stopped synthetic store is not a production backup rehearsal.
4. Keep [ADR 0134](../adr/0134-a-drain-before-every-restart.md)'s drain enabled for
   the Make deployment. For any later Ansible apply that can recreate services,
   explicitly drain again and account for running turns before applying. The
   role's optional drain unit covers host shutdown; its Compose task does not
   wrap a planned apply in drain/resume. Inspect the result because the existing
   drain helper deliberately fails open. An unaccounted failed drain stops this
   release even if the helper exits successfully.
5. Use the existing ansible-nas `make run-remote TAGS=endora` flow when applying
   NAS configuration: review its check/diff and approve the concrete changes
   before apply. It installs pinned collections and roles and pulls its checkout;
   record the resolved revision after that pull. Its dry run is not a quiet
   operation across all NAS roles, so keep the tag narrow. No generic full-NAS
   apply is part of this collaboration release. Review output privately because
   rendered host settings can contain credentials.
6. Confirm final service image identities, `/health`, HQ and runner versions,
   preserved records, lifted drain and live smoke results after the last apply.
   If Ansible changed running services after `make deploy-check`, repeat the
   relevant smoke checks against that final state. Enable only the approved
   conversation after these gates; installation alone does not enable assistance.

Ansible's normal confirmation is part of the established workflow. This packet
does not bypass it or grant new NAS network, firewall, storage or credential
changes. Such changes require the proposal process in ansible-nas's AGENTS.md.
The isolated local diagnostic remains an earlier gate; its supervised worker
dispatch does not replace the normal deployed runner.

## Fixed local model

The installed Endora Mac helper uses `llama-server`, rather than Ollama. Its
configured GGUF exists. Reading metadata and hashing files did not load the model.

| Item | Frozen value |
|---|---|
| Runtime | llama-server 0.7.0, build 11669, commit `620d2225c`, Darwin arm64 |
| Runtime SHA-256 | Recorded in the private manifest; recheck before activation |
| Model | `Swift-1.5-Qwen3.8-27B-GSQ-RCO-IQ3_S-mtp.gguf` |
| Model bytes | 12,120,016,896 |
| Model SHA-256 | Recorded in the private manifest; recheck before activation |
| Parallel workers | 1 |
| Pilot context | 8,192 tokens |
| Paid-call allowance | 0 microdollars |

The Mac has 32 GiB physical memory. Model loading and inference consume local
memory, GPU and time despite costing no API money; peak memory is unmeasured.
Use a separate loopback-only pilot model process with a fresh pilot key. Do not
stop, reconfigure or borrow the production helper's server or key. If insufficient
headroom exists for another process, stop preparation and report it; do not evict
the owner's work. Do not download a substitute model.

The exact proposed arguments are frozen in the private manifest. They preserve
the configured Jinja template, GPU settings, MTP, low reasoning effort and sampling
settings, with context reduced from 65,536 to 8,192 for this small fixture. Record
these as pilot settings, not as the production model configuration. Changing the
model, binary or these settings creates a new candidate and requires a new review.

## Scope and success criteria

One diagnostic request, in one conversation between `Member 2` (requester) and
`Member 3` (expert), charged to synthetic project `P1`. A lead is a support record
only, outside the two conversation participants. Use returned IDs, never assumed
SQLite row numbers. Persist the handle-to-ID map before the diagnostic starts.

The immutable original document `D1` contains exactly:

```text
health-check-v1: expected 200, observed 503; dependency fixture-cache is unavailable
```

The objective is: "Explain why health-check-v1 fails. Cite the granted original
document. Do not claim a repair or perform any action."

1. The real expert reads the granted original and answers once, identifying the
   unavailable dependency and citing the exact document. The requester reads
   the same original in its assigned result turn and independently checks the
   expected 200, observed 503 and unavailable dependency. A model's assertion
   that it read something is insufficient: inspect successful tool records and
   the actual immutable document/revision.
2. Exactly one logical request and accepted answer survive, with at most one
   result-run association. Expert and result runs retain `P1` in the authoritative
   ledger and spend zero paid microdollars. Count all attempts and model calls,
   including failures; zero dollars is not zero execution.
3. No deployment, repair, task acceptance, owner reassignment, recursive
   assistance, unrelated source access or external capability is authorized.
   The feature can be disabled and records remain inspectable. Unsupported or
   missing evidence is `cannot-tell`, never a successful run.

An operator checks these predicates independently against the fixture and
persisted tool/queue/ledger records. Compare the structured facts with the original;
do not compare the whole answer to an expected sentence or score the model's own
confidence. Do not call this task verification or accept a formal task from it.

## Activation sequence after approval

These are ordered preconditions. A failed precondition prevents the first model
call; approval does not waive a failed check.

1. Record the owner's approval, its scope and timestamp in the manifest. Recheck
   all fingerprints and verify the store remains stopped and feature-off. Make
   an offline copy of the stopped preparation database. Use fresh isolated node
   storage, credentials and explicit loopback ports, with inherited environment
   cleared. Never use the normal deployment command or its default compose stack.
2. Prove the network fence for these pilot processes before loading the model:
   only the selected pilot HQ, node and model listeners may be reached. Test a
   denied external destination and a denied nonpilot local listener; absence of
   cloud credentials alone is not a network fence. Record the actual isolation
   mechanism and negative results. No production socket, Docker socket, repository
   checkout, Vault, household integration or production address is mounted/given
   to a worker. If a working fence cannot be established, stop before inference.
3. Start the copied HQ and isolated node, still stopped. Ordinary non-demo HQ
   startup seeds a CEO, wing work, routines and cloud-brain defaults (see
   [HQ startup](../../app/hq/src/main.rs)). A fresh database therefore does not
   imply a two-member-only team. Keep all seeded IDs out of dispatch. Disable
   seeded routines using the existing owner API. Create the two synthetic
   participants and project through owner APIs; set work brains explicitly to
   `local`, runtime to `worker`, and project cap to zero. Judgment defaults can
   still be cloud: the dispatcher must refuse every cloud run, including support
   and seeded runs. Never change global guards to make admission pass.
4. Configure the isolated node's local relay to the pinned loopback model. Its
   cloud provider list and external integrations must be empty; the worker's
   contracts listing must offer no external capabilities. Use the existing node
   relay so actual turn checks and usage accounting run. Do not point a worker
   directly at the model or replace the relay with a fabricated zero-cost log.
5. Use supervised, sequential dispatch through the existing runner API, not the
   general Docker runner. Before starting a copied worker, check its live run's
   member, task, wake, brain, runtime and shell flag against the frozen map and
   permitted stage. Unexpected work stops the pilot without launching a worker.
   Workers receive only their short-lived turn token and isolated URLs, no shell
   or workbench. The runner credential is newly generated for this pilot.
6. In a synthetic fixture setup turn, use the existing authenticated document
   tool to create `D1` and its actual Internal source-run lineage. End the setup
   turn and record its calls separately. Do not insert document rows, fake receipts
   or source classifications directly with SQL. Setup is bounded to one local
   turn and cannot grant broader access. If valid setup cannot be admitted
   through the existing APIs, stop and investigate rather than weaken policy.
7. Enable conversations, create `C1` with the two explicit IDs and `P1`, then
   grant only `D1` through the existing owner source API. Record the immutable
   revision and grant expiry. Admit one synthetic requester turn with the frozen
   objective; verify its `request_assistance` acceptance and finish that turn.
   Permit only the resulting linked expert and requester result runs afterward.
8. The request has its existing 15-minute deadline, six expert read rounds and
   three total physical attempts per association. The entire session, including
   setup, has a 20-minute watchdog and one worker at a time. The supervisor also
   caps the session at eight physical worker starts (one setup, one requester,
   up to three expert and three result attempts) and 48 model calls in total.
   These additional experiment caps must be enforced before dispatch/call,
   rather than inferred from the final transcript. Automatic retries
   consume those bounds; do not start a fresh request to rescue a failed result.
   Record authoritative request/run IDs, all starts and model calls, timestamps,
   source reads, answer references, usage and ledger entries.
9. Disable conversations through `POST /v1/conversations/enabled` with
   `{"enabled":false}`, then stop the isolated team through `POST /v1/stop`.
   Verify both actions persisted. Inspect retained history and reconcile records
   before terminating only the recorded pilot PIDs. Retain the stopped store,
   immutable manifest and sanitized report for research; do not auto-delete them.

Owner operations use the fresh pilot owner credential, never a worker token.
The runner uses its separate credential. These are existing project interfaces,
not new production endpoints. Node sign-in and proxy verification are recorded
separately if served UI is exercised; a signed HQ bootstrap alone does not pass
the served-interface or physical-device gate.

## Stop conditions and remaining work

Stop immediately for an unexpected run, unauthorized source, missing accepted
intent, duplicate completed work, nonzero/unattributed paid charge, disabled
boundary, missing isolation evidence, changed fingerprint, memory pressure or
any execution bound breach. First disable new assistance, then stop workers and
the team. If the API is unavailable, terminate only the recorded pilot processes
and retain the database for reconciliation. No automatic restart or old-binary
rollback; forward disable remains the supported rollback for this candidate.

Current verdict: **prepared; live result cannot-tell**. Model loading, network
fence, tool-capable inference, complete source setup, result verification and
cost/latency reconciliation have not run. They must not be reported as passing.
This packet's approval covers the isolated activation sequence and one diagnostic
only. Phase 4's matched legacy-consult and manager-relay comparison still needs
its own frozen run scope. Phase 5 served-device visibility, Phase 6 evaluation,
production backup/restore and deployment gates remain in the
[reviewed plan](agent-collaboration-plan-reviewed-2026-10-21.md).
