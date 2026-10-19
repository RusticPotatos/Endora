# The team's standards

The base principles every project of the owner's is built by, every day: for the people who
work on them and for the agents (assistants and HQ's members) alike. Each project's own
`AGENTS.md` builds on this file; it never repeats it.

## Purpose and how this file is owned

- **One base, many projects.** These are the habits that hold in every repository. What is
  particular to a project lives in that project's `AGENTS.md`, never here.
- **The owner approves every change.** Nothing here changes without the owner's yes.
- **The team may propose.** HQ's members, and anyone else, propose a change as a branch with
  the evidence behind it: what went wrong or right, where, and what the change would have
  caught. A proposal without evidence is an opinion and waits.
- **Endora's repository is canonical.** This file lives at `docs/standards.md` in Endora;
  other repositories link to it rather than copying it, so there is one text to keep.

## Base principles

### Test first (TDD)

Write the failing test that defines the behaviour, watch it fail for the right reason, then
make it pass and tidy. Every behavioural change comes with its test; a bug fix starts with
the test that reproduces it.
*How you'll know:* the test was red before the code that turned it green.

### Behaviour in the owner's words (BDD)

What the owner decided is written as scenarios, in the owner's words, and run against the
public use cases: cucumber `.feature` files in Endora, molecule `verify` in ansible-nas
(a role starts, serves, and stops). Scenarios describe what is wanted, not how it is built.
*How you'll know:* the owner could read the scenario and say "yes, that is what I meant".

### SOLID

One reason to change per module; extend by adding, not by editing what works; a substitute
honours the contract it stands in for; small interfaces a caller actually uses; depend on
the port, not the vendor behind it.
*How you'll know:* a change to one responsibility touches one place.

### Abstract only when needed

Build what the current slice needs. The rule of three: duplicate once, abstract on the third
real use. No speculative layers, plug-in points, or configuration nobody asked for; no new
dependency without a reason written down.
*How you'll know:* every abstraction has at least two callers today.

### Domain first (DDD)

Organise by responsibility: bounded contexts, each with its own model and its own words
(the ubiquitous language: the names the owner uses are the names in the code). The domain
stays pure: no HTTP, database, vendor, UI or OS in it. Contexts talk through application
ports, and the dependency graph stays acyclic.
*How you'll know:* the domain compiles and tests with nothing but the shared kernel.

### Agile: small vertical slices

Deliver a thin slice that works end to end over a broad, shallow scaffold. Working software
at every step; show the owner early and change course on what they say. A slice is done when
it runs where it will run, not when it compiles.
*How you'll know:* the owner can use what was built this step.

### Happy path at the root

Prefer early returns over `else`; keep the main path of a function unindented and the
exceptions at the top. Match the surrounding style, naming and comment density.
*How you'll know:* a reader finds what the function does without unwinding branches.

### Models propose; deterministic policy authorizes

A model's output is a proposal. Whether a consequential action happens is decided by code
that does not depend on the model: the model is never the enforcement boundary, and no
privileged capability is handed to a model directly.
*How you'll know:* every refusal that matters is a line of code with a test.

### Evidence over claims

"Done" means shown, not said: a receipt, a reading, a test run, a diff. Work is checked by
someone other than whoever did it, against criteria written before it started (ADR 0114).
*How you'll know:* every claim of done cites what shows it.

### Honest reporting

Say what ran, what failed and what was skipped. Never claim a command succeeded without
having run it and seen the result. Separate evidence from inference from guess.
*How you'll know:* a report could be checked line by line against the record.

### Nothing personal in tracked files

No real names, addresses, places, network addresses, phone numbers or credentials in
anything git carries, and above all not in test fixtures. Use `John Doe`, `New York`,
`192.168.1.10`, `@example.com`. Real values stay in git-ignored local files, and a check
proves they are not in the tree.
*How you'll know:* the personal-values check passes, and a diff reads as anyone's.

### Security by default

Least privilege: each component gets the narrowest authority that does its job, and that
authority is revocable. No secrets in git, ever; credentials live in ignored files or the
host's secret store. Prefer reversible actions; escalate the irreversible.
*How you'll know:* nothing in the tree would help someone who stole it.

### Small, coherent commits

Conventional Commits (`feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`). A few
coherent commits over many tiny ones, never unrelated changes in one vague commit. Branch from
the project's base branch; never force-push or rewrite published history. AI assistance is
disclosed.
*How you'll know:* each commit's subject says truthfully all it does.

### Edits that fail loudly

Change files with tools that refuse when their anchor is missing or ambiguous. A scripted
replacement that matches nothing reports success and does nothing. Where only a script will
do, assert the anchor exists and re-read the result.
*How you'll know:* no edit was ever assumed to have landed.

### Decisions are written down

An architectural decision gets a decision record (ADR): context, decision, consequences. When
the architecture changes, its documentation and ADRs change in the same branch.
*How you'll know:* a newcomer can find why the system is the way it is.

### Architecture is settled by evidence, not by asking

Before putting a design question to the owner, read what the project already knows: its research
notes, its ADRs, and the designs it borrows from. Decide from them, and name them in the ADR. The
owner is asked only what evidence cannot settle: money, keys and accounts, accepting a risk,
irreversible steps, and taste.
*How you'll know:* the owner's questions are about what they want, never about how the system
should work.

### Think it through before running it

When something is going to be verified live, spend the minute reasoning first: a checker
that cannot fail, or a comparison against model output on exact strings, costs far more to
discover than to think about.
*How you'll know:* the check was shown to fail on a broken input before it was trusted.

### Every project has a map

Each project says what each folder owns and where new code, settings and docs go. New things go
where the map says; when nothing fits, the map is extended in the same change, never a new spot
invented.
*How you'll know:* nobody has to ask where something lives.

### One problem at a time, and stop the loop

What "done" means is written first: one to three checkable criteria. One task per problem; a
sub-task says which criterion it serves. When the same attempt has failed three times, stop and
report what was tried instead of trying it a fourth way.
*How you'll know:* no thread grows long without the work changing.

### "Can't tell" is an answer

When something cannot be verified, say so, with what was missing. A guess presented as a reading
is worse than an honest unknown, and a check that cannot see its inputs reports that it cannot
tell, not a pass.
*How you'll know:* every verdict is pass, fail or can't tell, never a pass by default.

### Checks you can trust

Zero warnings: none new, and fix the ones in code you touch. Judge a run by its exit code, not
its output, and chain checks with `&&` so a failure stops the commit. A failing test is not
believed until it fails alone, on a quiet machine, after a clean build. A change you can see is
run and looked at, not only compiled. Real captured data beats an invented fixture.
*How you'll know:* a green gate means working code, and a red one means a real failure.

### Long jobs are tracked

Builds and test runs that take minutes run as tracked background jobs with a timeout, never as
a hand-rolled polling loop that can outlive everyone watching it.
*How you'll know:* nothing is still running that nobody knows about.

### Plain text

No emoji or pictographs in a project's own text. New code, comments and docs are plain ASCII
where they can be (`->`, not an arrow character), and no em dashes: they read as machine
written. What a user sees may use what the interface genuinely needs, and icons come from the
platform's icon set, never character literals.
*How you'll know:* a plain-text check passes, and nothing reads as generated.

### Risks first, then act

Name the risks of a step before taking it. Look at what is there (`git status`) before anything
destructive. A pull request is opened only when asked.
*How you'll know:* nothing irreversible happened by surprise.

## Layering

1. **Global: this file.** The base every project shares.
2. **Each repository's root `AGENTS.md`.** That project's architecture, workflow and checks,
   building on this file and linking to it. `CLAUDE.md`, where a tool wants one, is a symlink
   to (or a one-line pointer at) `AGENTS.md`: one text, never a copy.
3. **Nested `AGENTS.md`, only where an architectural boundary needs guarding** (a client that
   speaks only over HTTP, a domain that must stay pure). Not one per folder.

A nested file never contradicts its parent. Precedence is "most specific wins", on its own
subject only: a nested file may add to or tighten its parent's rules for its folder, never
loosen them, and says nothing on subjects it does not own.

## Projects at a glance

**Endora** (this repository). A local-first personal butler: a Rust modular monolith organised
by responsibility, one crate per bounded context under `domains/`, composition roots under
`app/`, and the team's HQ among them. Base branch `develop`. Its root
[AGENTS.md](../AGENTS.md) holds the architecture rules and the checks to run before handoff.

**The handheld terminal** (`clients/terminal` in Endora). A small client that talks to the
node only over its HTTP API and imports no server crates. Its own
[AGENTS.md](../clients/terminal/AGENTS.md) guards that boundary, its privacy checks and how a
device install is verified.

**ansible-nas** (the owner's lab; the engineering wing's repository). The owner's public fork
of the upstream ansible-nas project, which builds the home lab. Base branch `develop` (`main`
mirrors upstream). It keeps upstream's conventions: a new application starts from the
`hello_world` template role; variables are named `<app>_*`; one documentation page per
application; a molecule scenario per role; lint pinned at ansible-lint 6.14.6 and yamllint
1.30.0. The team works there through `hq/` branches the owner reviews and merges, and never
touches `group_vars`, `host_vars` or inventories: they hold the owner's own values.

## Changing this file

- Propose a change as a branch (from HQ, an `hq/` branch) that edits this file alone, with the
  evidence in its description: what happened, where it is recorded, and what the change would
  have prevented or made easier.
- Keep it short: a principle is a few lines and a "how you'll know". If a rule belongs to one
  project, it goes in that project's `AGENTS.md` instead.
- A new project gets one paragraph under "Projects at a glance" when it starts.
- The owner approves and merges; until then the current text stands.
