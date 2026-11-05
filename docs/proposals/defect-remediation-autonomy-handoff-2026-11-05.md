# Defect remediation and autonomy handoff (2026-11-05)

Status: final. Every disposition below is the researcher's (its round-9 decision).
The owner's brief and every round's verbatim decision and evidence are kept outside git, in the
research folder (`.local/research-climb/defect-remediation/`, rounds 0–9); the two earlier climbs'
ledgers and results there are unchanged.

This assignment repaired what the two autonomy experiments proved defective — the agentic-harness
climb (rounds 0–23) and the minimum-viable-autonomy climb (rounds 0–5) — and prepares the next
climb to start from the corrected engine. Roles: an OpenAI reasoning model as researcher
(classification, acceptance criteria, independent review, every keep/revert decision); Claude
Opus as worker (reproduction, implementation, tests, reports). The worker never graded its own
results. HQ model spend: **$0** of the $10 allowed.

## The released baseline

**Live build `ea72ecf0`** (from `420cc722`), released 2026-11-05 through `scripts/release.sh` on
the researcher's approval: fresh gate (2,267 unit tests, 283 scenarios, 0 failed), personal-values
check, push, rollback tags on both hosts, both databases backed up and checked non-empty, strict
drains with 0 turns running and 0 cut, NAS smoke passing, and NAS, HQ and the Pi's runner all
reporting `ea72ecf0`. Eleven commits: A `1a445775`, `1c6a05d0`; B `4af2311e`, `2343a8b1`; C
`fae5596d`, `279ff4e6`, `b81b3708`, `9e2e18f5`; O11 `33d1fcc6`, `ea72ecf0`.

**Apparatus for the next climb:** branch `apparatus-v6` (local, not pushed) = the released engine
merged under the harness rig (v5, contract 2) plus protocol v6 (`b66e3c35`), accepted by the
researcher. Its development manifest, contracts and graders together, has a combined hash
beginning `127fe61c712f` (exposed seeds 0–3; the full manifest is `MANIFEST.sha256` in the research
folder's round 9). That manifest is development evidence, not a confirmation manifest (see the
freeze order below). The live engine remains `ea72ecf0`; the apparatus changes no engine code.

## Status words

- **Resolved and released** — fixed, independently validated, and through the release.
- **Fixed but unvalidated** — code exists and passes tests; the required validation is missing.
- **Investigated, unresolved** — cause or remedy not established.
- **Deferred** — deliberately left for later, with the reason.

## Dispositions

| ID | Finding | Source | Class | Status | Next climb may treat as resolved? |
|---|---|---|---|---|---|
| A (= H1) | Path guard refused ordinary `inventory/` and vault-named software paths | harness r23 (2 failures); autonomy r1 (4/8 harmless paths refused) | engine defect | **resolved and released** | yes, for the scoped defect |
| B | An owner question batched with a turn-ending `set_status` was silently dropped | autonomy r2 (task #1144) | engine defect | **resolved and released** | yes; Claude-specific reliability and natural frequency unmeasured |
| C | A turn interrupted after committing and before submitting lost its repair on resume | autonomy r4–r5 (2 of 8 interrupted items) | engine defect | **resolved and released** | yes, at the tested boundaries; general recovery reliability unmeasured |
| O11 | `scripts/release.sh` and the deploy's drain did not fail closed | remediation r5–r7 | apparatus | **resolved and released** | yes |
| O1 (= H2) | Asked the owner instead of reporting an impossible task blocked | harness r23 (1 failure) | model decision, provisional | investigated, unresolved | no |
| O2 (= H3) | Searched for files without calling `repo_checkout` (tool_failure) | harness r23 (2 failures) | model decision; cause unproven | investigated, unresolved | no |
| O3 | Excessive exploration and closing calls | autonomy reports | model decision, provisional | deferred | no |
| O4 | Proposal-level work credited as an applied outcome | autonomy r1–r2 | apparatus; non-delivery is policy (ADR 0115) | deferred | addressed by the protocol below |
| O5 | An evidence receipt refused, then retried successfully | autonomy reports | policy | deferred (no defect) | yes |
| O6 | `set_status in_review` accepted citing only a checkout receipt after a refused submission | remediation r1 | unresolved | investigated, unresolved | no |
| O7 | One anonymous failed `git ls-remote` to github.com while the rig was set up | remediation r1 | apparatus | containment controls demonstrated; incident retained | controls must be reused |
| O8 | Outside-repository briefing still says never edit "any inventory" | remediation r1 | policy | deferred: **prerequisite** | no — correct before the freeze |
| O9 | `workflow_paths` skips vault-named workflows in outside repositories | remediation r1 | unresolved | deferred | no |
| O10a | Suspected transcript loss when a turn ends by asking the owner | remediation r3 | unresolved; suspected engine defect | investigated, unresolved | no |
| O10b | The runner's late `ended` after such a turn is refused (409) | remediation r3 | policy | deferred (explained) | yes |
| H4a | Regression-test additions allowed in the needle family only | harness r19, r23 | apparatus | resolved in the accepted local apparatus v6 | yes (no engine deployment needed) |
| H4b | Needle precision missing from the task words | harness r19–r20 | apparatus | resolved in the accepted local apparatus v6 | yes, if contract 2 is selected |
| H4c | Grader defects: decoy mentions, test/method names | harness r13–r14 | apparatus | resolved in the accepted local apparatus v6 | yes; keep the controls and historical versions |

### A — the path guard (H1)

- **Reproduction.** On `420cc722`, fixture paths `shopkeep/inventory/stock.py`,
  `inventory/models.py`, `app/vaultwarden_client.py`, `pkg/vaulted_cache.py` in an outside
  repository: refused at node submission (422) and at HQ review.
- **Cause.** `endora_kernel::lab::forbidden_path` applied in every repository; ADR 0115 had
  accepted that limit.
- **Correction.** `refused_as_owners_values(path, outside_lab)`: the lab fork's rule in the fork,
  none outside it. `outside_lab` is trusted context only (the repository the owner set on the
  project, never a member's word); node submission and HQ review use it alike. The node refuses to
  mirror the lab's own fork as an outside repository. A filename rule only: it does not say an
  outside repository holds no secret. ADR 0115 amended.
- **Verification.** Red→green scenarios and unit matrix; nine protected shapes still refused in
  the fork. Live, offline (real HQ and node binaries): baseline refused, candidate took the outside
  branch to review; the lab path refused by both; HQ's second check still sends back `group_vars`;
  fork-as-outside refused.
- **Remaining risk.** No path rule at all in outside repositories (O8, O9).

### B — the dropped escalation

- **Reproduction.** On `420cc722`, `set_status in_review, ask_owner` in one reply: HQ received only
  the status change; the question never reached the desk (offline scenario and live run).
- **Cause.** The worker stopped at the first turn-ending call and left the rest unanswered.
- **Correction.** A reply whose turn-ending call is not its only one and its last runs nothing; every
  call is refused with why (`4af2311e`, the climb's `cecb173e`). Amended (`2343a8b1`): the refusal
  says a turn-ending call ends the turn the moment it runs and that an owner's decision is asked
  first. Refused calls never enter the repeat guard; an unchanged resend of a question HQ itself
  refused is answered by the guard. ADR 0109 amended.
- **Verification.** Live rig (real HQ and worker under sandbox, a stand-in node relay, the
  conflicting reply scripted, every later reply from a real local open-weight model): `cecb173e`'s
  text alone 9/10 (one model sent `set_status` meaning to ask next); amended 21/21, final build
  10/10, no duplicate, no premature review, one refusal per run.
- **Limits.** Deliberate exposure; an open-weight model, not Claude; natural frequency untested.

### C — committed work lost after interruption

- **Reproduction.** The climb's witnessed losses (2 of 8; baseline 1 of 2 in round 5) and offline
  red scenarios: no submission state in a retried turn's briefing or kept checkout; a revision
  submitted again recorded as a new submission (second check, gate shut, review voided).
- **Cause.** Missing submission context is the supported contributor to the historical losses:
  resume context said only "Go on from it". Separately, HQ's record of a branch was not
  idempotent; that was reproduced offline as a recovery hazard (a turn that resubmits would void
  its own review), not shown to have caused the original losses.
- **Correction (no new storage).** HQ tells a retried turn, or any turn on an item with a
  submission, its newest submission acknowledged as finished (branch, full head, status), any newer
  work-in-progress head apart, or that none is acknowledged and the node may hold one; a commit is
  not a submission. The kept checkout names its full head. HQ records the same branch and full head
  as its newest finished submission as nothing new, whatever was kept in progress since. ADR 0115
  amended.
- **Verification.** Live interruption rig (real HQ, node and worker, local model, witnessed kills,
  a proxy making the node-to-HQ windows), every candidate window with a second kill on resume:

| Window | Candidate | Baseline |
|---|---|---|
| before commit (uncommitted work) | 1 commit, 1 finished submission, in review | same |
| after commit, nothing submitted | same, plus one work-in-progress keep of the same head | same (recovered) |
| node took it, HQ never heard | 1 finished submission | same |
| HQ recorded it, answer lost | 1 finished submission, "Submitted" posted once | same |
| after acknowledgement | 1 finished submission | same |

  This model recovered on the baseline too, so the live runs show correct recovery and no
  regression, not an improvement rate. One episode was voided (the model server stopped) and kept.

### O11 — the release path

`release.sh` carried on past failed tags, failed or empty backups and a failed NAS deploy, and
`hq-drain.sh` deployed anyway when it could not drain. Now every stage stops the release, the gate
and the personal-values check precede the push, both backups are checked, both hosts must report the
build, and a release's drain is strict (`DRAIN_STRICT=1`: a refused drain, a cap that passed with
turns running, `NO_DRAIN`, or an HQ that is down or old stops it before any restart). The deploy
recipes set `pipefail`. Controls: `test-release.sh` (38), `test-hq-drain.sh` (38),
`test-deploy-recipes.sh` (16, the real `make` recipes against fakes). ADR 0134 amended.

### The unresolved and deferred findings

No correction was made for any of these. Evidence pointers are to the research folder.

| ID | Reproduction | Cause, or the uncertainty | Evidence reviewed | Remaining risk | Next action |
|---|---|---|---|---|---|
| O1 | Not reproduced here; one confirmation failure (impossible-104729, a timeout after asking) | Model decision, provisional: no engine contribution shown (blocking was available) | agentic-harness round 23 | A task the model should block may wait on the owner | Observe in the next climb; correct only on a demonstrated engine contribution |
| O2 | Not reproduced here; two confirmation failures (tool_failure-104729, -155921, "blocked") | Unproven: inherited context is plausible, not demonstrated | agentic-harness round 23 | A turn may look for files it never checked out | Inspect the actual briefing and tool list on a fresh failure before any change |
| O3 | Not reproduced as a consequential failure | Model decision, provisional | minimum-viable-autonomy reports | Cost, not correctness | Act only on a concrete unnecessary call with a consequence |
| O4 | Reproduced by design: outside-repository work is never applied (ADR 0115) | Apparatus: the old grader credited proposals; non-delivery itself is policy | minimum-viable-autonomy rounds 1–2 | Outcome claims without application | Build and validate the apply-and-verify path below before any outcome claim |
| O5 | Observed once: a receipt refused, then a corrected one taken | Policy enforcement working as designed | minimum-viable-autonomy reports | None shown | None; reopen only if a valid receipt is refused |
| O6 | Seen once in a rig run (remediation round 1, lab case): `in_review` accepted citing only a checkout receipt after the submission was refused | Unknown whether a branch was a required criterion and whether acceptance or delivery can advance without one | remediation round 1 records | Review on evidence that does not show the change | Establish the evidence boundary for review, acceptance and delivery |
| O7 | One anonymous, failed `git ls-remote https://github.com/example/shopkeep` before the rig's environment rewrite (no credentials, the repository does not exist) | The node's git ignores the global config file; the rewrite had to go through `GIT_CONFIG_*` | remediation round 1; containment probes in rounds 2–5 | A rig that leaks to the network | Reuse the sandbox, the environment rewrite and the dead proxy for every process |
| O8 | Present in source: an outside checkout's text forbids "any inventory" and carries the lab's framing | Policy: lab-only wording given to every repository | remediation round 1 | Confuses work in outside repositories | Prerequisite below: correct and test before the next freeze |
| O9 | Present in source: `workflow_paths` skips vault-named workflows in outside repositories | Unknown whether it ever omits evidence a CI criterion needs | remediation round 1 | A CI criterion judged without its workflow | Act only on a reproducer that loses needed evidence |
| O10a | Suspected transcript loss: on the stand-in relay, HQ refused the end-of-turn transcript of every turn that ended by asking the owner (HQ ends the run as the question lands) | Not confirmed on the real node | remediation rounds 2–3 records | Such turns may keep no transcript | Confirm on the real node; then decide the retention contract |
| O10b | Every such turn's late runner report got 409 | Explained: the run had ended and its lease was removed | remediation round 3 | None shown | None |

### H4 — the apparatus

`apparatus-v6` carries rig v5 and contract 2 forward (H4b: the needle's precision sentence is in
the words; `test_probes.py` checks it) and adds protocol v6 (H4a): a pure test addition is allowed
in every family whose words do not forbid touching the tests or the repository (five_file_rename,
needle, interrupted, already_satisfied, unambiguous, three_subtasks), by v5's needle rule; the four
others still fail; a changed or weakened test fails everywhere. Controls in every family
(`test_file_added`, `test_appended`, `test_weakened`); the tests hold v6's changes from v5 to exactly
the declared ones, keep v5's verdicts unchanged, and keep the round-13 decoy and name controls
(H4c). All 72 probe tests and the rig's other suites pass. Round 23's frozen verdicts stand as
graded. `freeze.py` writes the task contracts beside the graders and hashes them together. The
researcher accepted v6 and recomputed every manifest entry independently.

## Review of the minimum-viable-autonomy protocol (brief §6)

Findings are what the records show; readings are inference.

| Question | Finding | Reading |
|---|---|---|
| Why only three interventions? | Round 0 set "maximum three intervention experiments, then one frozen confirmation", with a 12-hour box | A guard against fitting small samples. It bounded the experiment, and it exhausted investigation early once live validation was blocked |
| Why $0.124 of $10? | Phase allocations ($2 baseline, $3 development at ≤$1 an intervention, $4 confirmation); an episode cost about $0.01–0.02 | Neither the budget nor the deadline was binding at STOP; the confirmation money was reachable only with an eligible candidate |
| Did the 12-hour box matter? | STOP at 09:05 UTC with about 3h50 left | Not binding at STOP; it shaped the plan toward few, small rounds |
| Was the count useful when a candidate could not be live-validated? | Round 3's candidate used an intervention; it established the mechanics (scripted scenarios, full gate), while its behavioural validation was unavailable; it was validated here at $0 | A count spent without the means to judge the candidate behaviourally is a weak stopping signal |
| Apparatus repair vs product intervention? | Round 2's grading repairs were kept as measurement, uncounted; framing paragraphs counted | The distinction existed for grading; rig and isolation work should be named and uncounted too |
| Should a blocked experiment consume an allowance? | — | No: record it as blocked, with the minimum arrangement it needs |
| Did the stop rule reward stopping early? | "Three unsuccessful interventions" was a sufficient stop condition by itself | In effect, a count stopped the climb rather than evidence that further investigation was unproductive |

The researcher followed the rules as written; the rules, not their application, constrained
discovery.

## Proposed next minimum-viable-autonomy climb (brief §7)

Not started here.

### Prerequisites

1. The released engine `ea72ecf0` (done).
2. **O8 corrected and tested:** a checkout of a repository outside the lab carries no lab-only
   prohibition ("never edit … any inventory", the ansible-nas framing); the fork's checkout keeps
   it. Test: a worker scenario that an outside checkout's text omits it and a fork checkout's
   includes it, and a team scenario for the briefing's lab paragraph on an outside item.
3. Apparatus v6 (accepted), contract 2 selected, and a confirmation manifest made in the freeze
   order below and reported before any run.
4. The disposable arrangement built here, reused: real HQ, node and worker binaries on loopback;
   GitHub addresses rewritten to local bare repositories through `GIT_CONFIG_*` environment
   `insteadOf`; the worker under `sandbox-exec` (localhost only, writes only in the run folder);
   containment probes before every attempt; a dead proxy for leaks.
5. A model decision: the local open-weight model for mechanism checks; Claude only through a
   disposable, capped key when Claude-specific behaviour is the question (the owner's decision).
6. Its own approved budget and model choice, measurable success criteria, and confirmation and
   stop rules, decided before it starts. This proposal does not authorize running it.

### The freeze order

1. Freeze the candidate, the task generator and the graders: report their source hashes.
2. Then the researcher selects the held-back confirmation seeds, independently.
3. Then generate those seeds' exact contracts with the unchanged sources (`freeze.py --seeds …`),
   and report the final combined manifest before anything runs.

The exposed-seed manifest made here is development evidence, never the confirmation manifest.

### The minimum apply-and-verify arrangement

ADR 0115 keeps a branch of a real outside repository on the node, never pushed. The next climb
needs a change to take effect somewhere, and a witness to see it last, without weakening that:

- **The target:** a rig-owned, disposable environment (a local bare repository the rig serves as
  "github.com/example/…", and, for operations, a small service the rig runs) that exists only for
  the run.
- **The authorization boundary:** the owner marks that target disposable on the rig's HQ (a
  project setting on a disposable HQ only); accepted work is applied by the rig's own step, never by
  the agent, and only to a target so marked. Real outside repositories keep ADR 0115 unchanged.
- **The witnesses:** the rig, not the agent, reads the target after application and again after a
  later disturbance or task, and records what it saw.

### Five observations per episode

1. The agent decided correctly (graded against the frozen contract).
2. It proposed a change (a submission recorded by HQ and the node).
3. The change was authorized (the owner's or rig's step, recorded).
4. It took effect (the witness read the target).
5. It persisted and was verified (the witness read it again later).

### Protocol

- Same roles; the worker never grades.
- No cap on investigative rounds. Rounds are bounded by budget, safety and evidence: stop when
  evidence shows more investigation is unproductive, unsafe or unjustified.
- A repeated failed approach is bounded: after three attempts at the same hypothesis without new
  evidence, the approach changes or stops (the shared three-attempt rule); this bounds approaches,
  not the total number of experiments.
- Apparatus and isolation work is named as such and uncounted; a blocked experiment is recorded as
  blocked, with the minimum arrangement it needs.
- Enough exploratory attempts per hypothesis to tell a model limitation from an engine defect: the
  mechanism reproduced offline, then live.
- Measures: useful autonomy (observations 4 and 5), safety (zero unauthorized effects), restraint
  (healthy cases left alone), human burden (questions, interventions), execution cost (tokens,
  money, agent-active time), complexity added (lines, mechanisms).
- Fresh confirmation environments and seeds, chosen after the candidate and the manifest are
  frozen, never seen in development.
- The researcher may conclude that no new mechanism is needed.
