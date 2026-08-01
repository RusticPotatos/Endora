# 0054 — Other people's services

## Status

Accepted (2026-07-26). **Consolidates 0021, 0038, 0039, 0040, 0041, 0042, 0043 and 0045**
(and, through 0041, the archived 0046–0048). The largest cluster, because this is where
almost every real failure has come from.

## Context

Endora is only as useful as the things it can reach, and everything it reaches belongs to
somebody else: a smart home, a calendar, a filesystem. Those services expose tools designed
for a different consumer — usually a voice assistant — and the gap between that surface and
what a butler needs is where the work is.

The evidence, from one house over one week: nine attempts to switch a light that failed
because the model kept inventing names; fourteen consecutive attempts at a light called
`Kitchen Table` with `names: Kitchen Table` in the model's context every single time, and
not once did it send that string; a call that switched on **every light in the house**; a
tool that can only set brightness being chosen repeatedly to switch a light on.

The wrong response to all of that is to patch each one. [0038] counted the result of doing
so: six hardcoded quirks and one integration's response format parsed inside the generic
protocol client.

## Decision

### A catalogue, with configuration and enablement

Capabilities — built in, or discovered from **MCP servers** Endora hosts — live in one
catalogue with per-capability configuration, enablement and settings. MCP tools are
namespaced `server.tool`, deny-by-default, and the person opens them individually.
"Off" means **not offered to the model at all**, for every kind of capability, from every
source.

### Facts about a tool are data, ranked by trust

Endora keeps a profile per capability, built the way it builds understanding of a person —
facts with provenance, correctable — from three sources, **ranked**:

1. **Declared** — what the server says about itself. A proposal, never authoritative: a
   careless or hostile server says exactly the same words as a good one.
2. **Observed** — what Endora saw happen, accumulated from outcomes
   ([0053](0053-honesty-about-what-it-did.md)).
3. **Confirmed** — what the person said. Authoritative.

Deterministic policy consumes confirmed facts. Declared facts are surfaced, never obeyed —
[models propose, policy authorizes](0051-where-the-boundary-is.md) applied to a third party.

The facts worth holding are deliberately few, and they come from **one question**: *which
tool reads this server's state?* That nomination makes the reader, and makes every other
tool on that server verifiable through it.

### Findings are derived, never stored

From outcome history alone, Endora derives what is wrong with its own tooling. A proposal is
**computed on read** — there is no table, no row to dismiss, nothing to groom. When the
outcomes age out or the world changes, the finding stops being derived. This is
[0052](0052-what-it-knows-about-you.md)'s anti-queue guarantee made structural rather than
promised, and **answering is the dismissal**.

Two findings, told apart by how *wide* a failure is — arithmetic that names no service:

| history | means | remedy |
| --- | --- | --- |
| fails on one target, works on others | the name is wrong | *what is it really called?* |
| **refuses** on several, has **never** worked | the tool is wrong | *stop offering it* |

Only outright refusals support a withdrawal, and **any** success disqualifies it — including
an unverified one. The first version counted "changed nothing" too, and immediately proposed
withdrawing the most useful tool in the house on five successes misread as failures.

The withdrawal is applied by policy without asking
([0051](0051-where-the-boundary-is.md)); the name question is the person's and always will be.

### Finding the thing the person meant

When a call fails, Endora searches the service's own reading for what the call was aiming at.
It always shows what it found; it acts only when one candidate is clearly the answer. Four
signals, each from a specific failure:

| signal | asks |
| --- | --- |
| matched words | how much of the request does this name contain? |
| leftover words | how much of *itself* does the request not account for? |
| operability | could the tool actually act on this? |
| uniqueness | does exactly one candidate survive? |

The first two rank, the third can only remove a candidate, the fourth is the gate. A tie acts
on nothing. Everything is **pure text** — a reading is fragments, a value is what follows a
colon, a candidate shares whole words — so a calendar is searched by the same code as a house.

A filter value the service has never used as a **category** is treated as part of what was
named: "kitchen table" arriving as `{area:"kitchen", device_class:["table"]}` has no name in
it at all, and acting on what remained switched on two lights. The service supplies the
vocabulary; where there is none, nothing changes.

Recovery **never widens a call**: kind filters are kept, and only scalars the real name
already contains are dropped. Written after argument hygiene turned an aimed-at-nothing call
into every light in the house.

### An exact identifier pins the target

A call that already names one thing must not also name a room. Live, and it **succeeded**,
which is why nothing caught it:

```text
{entity_id: "light.kitchen_table", area: "kitchen"}
  -> completed successfully on: Kitchen (area), Kitchen Main Light, Kitchen Table
```

Two lights, for a request that named one. Every widening guard watches the *failure* path;
a call that widens and works is invisible to all of them.

So where a channel can tell that a call already pins its target, the broader targeting
fields are dropped **before** the call. This is the only place a channel acts ahead of a
failure rather than after one, and it is allowed for a single reason: it can only ever make
a call hit *less*. Kind filters stay, because dropping those is the widening this exists to
prevent.

### Direct reach, where it is given

A tool surface is a product decision, not the service. Where Endora holds the service's own
interface it uses it — to see **everything** (including what the tool surface hides) and to
act on exactly one thing **by id**, which cannot be mis-matched. All service-specific
knowledge lives in one named adapter; everything above it speaks to a generic port and never
learns which service it is.

Deliberately not "run arbitrary API calls": that is a far larger grant than any observed
failure needed.

### It can change the service's configuration, narrowly

Endora writes the names **the person confirmed** into the service that owns them, so the fix
works for every client rather than only for Endora. Strictly additive, never inventing a
name it merely inferred, off until deliberately enabled — and **every change is logged with
the prior value**, individually reversible. A prior value nobody stores is not a
reversibility story but a claim about one.

That log survives "forget everything": it is not knowledge about the person, it is a receipt
for a change that still exists inside somebody else's service, and deleting the receipt does
not undo the change.

### One thing that stands for many

Some requests cannot be aimed. "Turn off all the lights" produces a call aimed at nothing,
which is **indistinguishable** from a model that failed to say what it meant — so it is
refused, correctly, and the person cannot have what they asked for. Every mechanism here
makes calls narrower; a collective request had nowhere to go.

The answer is not to fan out across many ids at action time. That is the one move all of
this exists to prevent, and it half-applies when something fails partway.

Instead the service is asked to **hold a collection once**: a group, a list, whatever that
service calls one thing standing for several. After that there is nothing special about it —
it has a name and an id and is hit exactly like anything else. A dangerous runtime fan-out
becomes a one-time, reversible configuration change plus an ordinary single-target action.

It also means the person can see it. A group is visible in their own service, editable
there, and gone the moment they remove it.

**What is stored, and why.** A collection is created with **no prior value**, which reads
exactly like adding a name — so undoing it as one would replay an empty list and strip
every name off whatever it points at. Add-versus-remove stays derived, but the *kind* of
change is recorded, because those are different acts with different undos and guessing
between them is destructive.

### A quirk that widens is not a quirk worth keeping

One of the original per-integration patches dropped a `name` that was merely a device-kind
word (`{name: "light", area: "kitchen"}` → `{area: "kitchen"}`), leaving *every light in
the kitchen*. It predated the rule against widening, and it broke it.

Deleted rather than generalised. The search reaches a better answer without widening
anything: `Kitchen Main Light`, one thing, which is what a person calling it "the kitchen
light" means. It also took a hardcoded list of English device words with it.

The general lesson: a patch that survives long enough to be inherited should be re-read
against the rules that arrived after it. This one had become the thing it was protecting
against.

### The cost of a stack of decorators

Runners are layered — withdrawal over composition over search over aliases over openers —
and every port method has to be forwarded by every wrapper above the one that answers it.
A defaulted method that returns nothing is **indistinguishable from a service having
nothing to say**, so forgetting to forward one fails silently and looks exactly like
working correctly.

Both of the last two additions were dropped this way. Presence never reached a turn;
neither did the facts behind an answer. Unit tests passed throughout, because they
exercised the runner that answers rather than the stack that production builds.

Two guards, because a promise is not a mechanism. The first version of this section said the
composed-stack test "should be extended whenever a port gains a method" — which is a note
asking a future person to remember, and the whole problem is that forgetting is silent.

- **The forwarding is generated.** A decorator declares which of the port's defaultable
  methods it simply passes along; adding a method to the port means adding one arm in *one*
  place, and every decorator that lists it forwards it correctly by construction. What a
  decorator genuinely overrides it still writes by hand, right beside that declaration, so
  "changes this" and "passes this along" are visible together.
- **A test proves the chain is transparent**, by putting a runner at the bottom that records
  which methods reach it and calling every defaultable method at the top. This covers the
  runners that *aggregate* rather than wrap and so cannot use the generated form.

Writing the forwarding out by hand is what made the original bugs possible, and it had
already produced two more latent ones: two runners in the middle of the stack never passed
presence or states along, and got away with it only because the runner above them happened
to answer first. Reordering the stack would have broken it silently.

### Tools, skills, agents

One word has been doing three jobs. What the console calls a "skill" is a **tool**: one call,
one result. There is no layer above it, and the absence is why a brief could only ever be *a
prompt asking the model to be the missing layer* — which failed six times in a row.

| | what it is | who composes it | example |
| --- | --- | --- | --- |
| **tool** | one call, one result | nobody | `GetLiveContext`, `weather` |
| **skill** | a **procedure** over tools, toward an outcome | **code**, from a recipe | the morning brief |
| **agent** | chooses skills and tools against a goal | the **model**, within policy | the butler turn |

The distinction matters because it says where the model belongs. An **agent** decides *what
to do* — that is judgement, and it is what a model is for. A **skill** decides *how a known
outcome is assembled* — that is procedure, and deciding it afresh every morning is the
mistake. A brief needs no judgement about whether to include the calendar; it always should.

**Where dynamism belongs.** The worry about a fixed procedure is real: a skill that cannot
change is a skill that rots. But the axis is wrong. What must adapt is the **recipe** — what
goes in, on the scale of weeks — not the **composition**, decided fresh each time. So a
recipe becomes data, amended by evidence and the person's confirmation, which is
[0051](0051-where-the-boundary-is.md)'s *models propose, policy authorizes* applied to
procedure rather than to action. Execution stays deterministic until something says otherwise;
the mechanisms that would say so — reactions, derived findings — already exist.

**Not built yet, deliberately.** There is one recipe. A composition engine for a single
instance is the speculative abstraction this project refuses, and its shape would be guessed
from one example. The distinction is recorded now because it changes how the next thing is
built; the machinery waits for a second skill to argue for it.

### Auto-allow has to be reversible

Turning auto-allow **on** writes a per-tool `opened` flag for every tool a server exposes.
Nothing reversed them, so turning it **off** left them all open — twenty of them on one
server, including a tool that plays audio through the house. The setting was a one-way door,
and the way back out was twenty-one separate actions.

Turning it off now closes every tool on that server. **Every** one, not only those auto-allow
opened, because nothing records which was which — and that is the safe direction: the person
reopens what they want, where the alternative leaves something open that they believe they
have just closed.

The general rule: a switch that writes state elsewhere must unwrite it, or it is not a switch.

### Connecting a new kind of thing is the service's form, rendered

The most useful thing Endora could gain is not another integration written here — it is a
**calendar**, or a door sensor, or mail. Writing an adapter for each is the per-integration
patching this document exists to stop, one layer up.

But the service already knows what each of those needs, and will say. Home Assistant declares
885 setup handlers, each with its own form: field names, types, which are required, what to
default. So Endora **starts the service's own setup flow and renders whatever comes back**. A
kind of thing nobody here has heard of works exactly like one that ships today, and adding
support for something new needs no code at all.

The interface offers a few suggestions because 885 opaque names is not a menu — but they are
a convenience, not the supported set. Anything the service can set up can be typed in.

**Nothing typed is stored.** A credential travels from the person's keyboard to the service
that will hold it and is not written down on the way: no setting, no log line, no event text.
Endora is passing a message, not keeping an account — which is also why this is the *service's*
form and not one Endora designed. A field whose name looks like a credential is masked, a
heuristic that fails safe: wrongly masking a field still submits it correctly, while the
reverse puts a password on a screen.

What the service refuses is reported **in its own words**. *"invalid_auth"* is the entire
reason somebody is looking at that screen, and collapsing it into "it didn't work" would leave
them guessing.

### Quirks are allowed, behind a boundary

Some knowledge genuinely cannot generalise. It stays code — in that integration's **named
adapter**, never in the shared runner and never in the protocol client. The test: *could
another server reasonably need the opposite behaviour?* If yes, it is a quirk.

A channel reaches the rest of the system through three small pre-call questions, each
defaulting to "nothing to say" so a service that cannot answer changes nothing:

- **refuse** — this call cannot do anything, so do not send it. A call that quietly does
  nothing is the worst failure available: it reports success, changes nothing, and leaves
  the person and the record disagreeing.
- **tighten** — this call already names one thing, so drop what could only widen it.
- **categories** — these are the words this service uses for *kinds*, as opposed to names.

### What was cleaned up, and what was simply deleted

The six hardcodes this ADR's predecessor inventoried are now resolved, and two of them by
removal rather than relocation:

| hardcode | outcome |
| --- | --- |
| `is_state_reader` | became data — the person nominates the reader |
| `verifier` mapping | became data, from the same nomination |
| `drop_domain_word_name` | **deleted** — it widened calls, which the rules now forbid |
| `reject_no_op_light_set` | moved behind the adapter, as `refuse` |
| `flag_ambiguous_names` | **deleted** — one service's text format parsed in the *orchestration* layer, warning a model in prose about an ambiguity the search now settles in code |
| the response envelope in the transport | still there; the last one, and the largest |

`flag_ambiguous_names` is worth its own note. It parsed `names:`/`domain:` lines to tell the
model "one name means two things here — do not guess". Every part of that was wrong by the
time it was removed: the wrong layer, a service-specific format, and a *prose warning to a
model* standing in for a rule. The rule exists now — a tie acts on nothing — and it does not
depend on the model reading anything.

## Consequences

- Any MCP server can get read-back, evidence, findings and repair without Endora shipping a
  line of code about it.
- Onboarding an integration is a person answering one question, not a patch release.
- The name-matching failure class **ends** for any service Endora has reach into.
- **Four rules decide which thing a person meant.** Each came from an observed failure and
  each has a test against a real house. A fifth should be read as evidence that ranking names
  is the wrong instrument.
- Endora now holds credentials to a service and edits its configuration. The gates are:
  confirmed facts only, additive only, undo stored, off by default, disclosed.

## Rejected

- **Patching each integration.** Produced six hardcodes and a leak into the transport.
- **Trusting MCP annotations.** Taking an unvetted third party's word for what is safe.
- **Inferring tool behaviour from names** (`Get*`, `List*`). A heuristic that is usually right
  is the worst kind at a policy boundary.
- **Asking the model which of its tools are useless.** It is the component whose bad choice
  created the finding.
- **Storing findings so they can be dismissed.** A dismissible record is a queue with extra
  steps.
- **Writing names Endora merely inferred.** A recovery proves what worked for one call, not
  what a word means in a house.
- **Renaming a person's entities** instead of adding aliases. Destructive, and it presumes
  Endora's opinion beats theirs.
- **Requiring every word to match**, **preferring the shortest name**, **acting on the best of
  several tied candidates**, **dropping kind filters on retry**, **filtering non-controls out
  of the shortlist** — each tried, each wrong, each recorded in the archived originals.
