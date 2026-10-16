use super::*;

/// Assembles the [`ButlerContext`] — a snapshot of what Endora understands about
/// the person and the skills it can reach right now — so the butler's conversation
/// The reserved id of the lookup that fetches a deferred tool (ADR 0060).
///
/// Named for what somebody would say rather than for the mechanism: the model is choosing
/// a thing to do, not managing a catalogue.
pub const LOOK_FOR_A_TOOL: &str = "find_a_way_to_do_it";

/// How many proven actuators sit in front of the turn beside the readers.
///
/// A handful, not all of them: enough that the things this person actually does are one step
/// away, few enough that the list stays the short one the model chooses correctly from.
pub(super) const MOST_PROVEN_ACTUATORS_IN_FRONT: usize = 4;

/// A tool with no record yet, scored so it is neither trusted nor punished.
///
/// Absence of a decision is not a decision — the same rule that stops a standing default
/// re-opening a tool somebody blocked, pointed the other way. A brand-new tool outranks
/// one that has failed every time it was tried and is outranked by one that works.
pub(super) const NOTHING_KNOWN_ABOUT_IT: f32 = 0.5;

/// What the turn is offered, and what waits behind a lookup (ADR 0060).
///
/// A butler asked about events in a city was handed **37** tools — nine skills including one
/// that answers exactly that, twenty from the house, eight from a search engine — and replied
/// that "the functions provided are from Home Assistant". Nothing was unwired. Published
/// work puts the audit threshold around thirty tools and the degradation starting at twenty;
/// deferral moved Opus 4 from 49% to 74% **on unchanged weights**, and the model here is far
/// smaller than that.
///
/// The split is by what a tool **does**, not what it is about:
///
/// - a **read** is cheap, its result is evidence ([0053]), and it is useful far outside its
///   own subject — whether anyone is home bears on questions that have nothing to do with
///   the house. Reads stay.
/// - an **actuator** is only wanted once the person has asked for something to happen, so it
///   waits behind one lookup.
///
/// Deferred, never removed. When the guess is right the two are identical; when it is wrong,
/// a removed tool is unreachable and the model cannot even ask — it apologises instead,
/// which is exactly what happened. Missing on a paraphrase is the acknowledged failure of
/// every retrieval scheme including the ones that work, so the recoverable version is the
/// only one that survives being wrong.
///
/// **What lands in `deferred` is wider than "things that act", and the way back has to say
/// so.** A tool counts as a read only if it is the one the person nominated as that server's
/// state reader, and there is exactly one of those per server — a field that exists for
/// read-back verification, doing a job it was never shaped for. A search server with eight
/// read-only tools therefore has seven of them classed as actuators.
///
/// The first version of this described the lookup as the way to *"tools that can DO
/// something — turn things on or off, set, play, send"*, which made those seven unreachable
/// in practice: no model calls an actuator-finder to answer a question. Deferral is only
/// safe because it is recoverable, and an escape hatch nobody opens is not one.
///
/// Deferred tools come back **ordered by what has actually worked**, from read-back that is
/// already recorded. Every published approach ranks on a tool's description, which is why
/// they would all offer `HassLightSet` first forever: it reads perfectly for "turn off the
/// kitchen light" and has never once worked. They are stateless retrievers and never find
/// out. This one is told.
#[must_use]
pub fn offered_and_deferred(
    available: Vec<CapabilitySpec>,
    how_it_went: &std::collections::HashMap<String, (u32, u32)>,
) -> (Vec<CapabilitySpec>, Vec<CapabilitySpec>) {
    // Offered means runnable: the clearance gate refuses a read that has lost its
    // autonomy (a stance the person set to `off`), and a tool that is shown but
    // refused on every call is a trap, not an offer — the model reached for it
    // every time, was turned away every time, and the turn ended in apology.
    let (mut offered, mut deferred): (Vec<_>, Vec<_>) = available
        .into_iter()
        .partition(|c| c.reversibility == Reversibility::Observe && c.autonomous);
    let worked = |c: &CapabilitySpec| match how_it_went.get(&c.id) {
        Some((_, 0)) | None => NOTHING_KNOWN_ABOUT_IT,
        #[allow(clippy::cast_precision_loss)]
        Some((confirmed, tried)) => *confirmed as f32 / *tried as f32,
    };
    // Descending, and stable — so tools nothing is known about keep the order they were
    // registered in rather than being shuffled by a sort that has nothing to go on.
    deferred.sort_by(|a, b| {
        worked(b)
            .partial_cmp(&worked(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // The ones that have actually worked stay in front.
    //
    // Measured, and it is the correction this whole idea needed. Deferring *every* actuator
    // and trusting the model to open the way back failed the eval 0 times out of 3: asked to
    // turn a light off with nothing in front of it that could act, it did not reach for the
    // lookup. Deferral is only safe because it is recoverable, and a recovery the model
    // never takes is a deletion.
    //
    // So the record decides what is in front, not only what comes back. A handful of
    // proven actuators are offered directly — which is exactly the short, clean list the
    // battery already picks correctly from every time — and the long tail stays behind the
    // lookup where the crowding was.
    //
    // Nothing unproven is promoted. A tool with no record cannot buy its way to the front,
    // and one that has never worked certainly cannot.
    let mut promoted = 0;
    deferred.retain(|c| {
        let proven = how_it_went
            .get(&c.id)
            .is_some_and(|(confirmed, _)| *confirmed > 0);
        if proven && promoted < MOST_PROVEN_ACTUATORS_IN_FRONT {
            promoted += 1;
            offered.push(c.clone());
            return false;
        }
        true
    });
    offered.sort_by(|a, b| a.id.cmp(&b.id));
    (offered, deferred)
}

/// How many findings from one past reply are carried forward.
///
/// A handful: enough that "and what about next week?" has the list it is talking about, few
/// enough that a search result page cannot crowd out the conversation it belongs to.
pub(super) const MOST_FINDINGS_CARRIED: usize = 3;

/// The longest a single carried finding may be.
pub(super) const LONGEST_FINDING_CARRIED: usize = 400;

/// The conversation as the butler sees it — what was said, and what it found while saying it.
///
/// Every tool result inside a turn is threaded properly and then **thrown away at its end**.
/// A turn was seeded from prose alone, so asking the same thing twice started from nothing:
/// the butler had its own summary of what it said and no trace of what it read. It looked
/// again, or gave up and asked the person to say more — which is what asking twice felt like.
///
/// The findings were already stored, for the person, so a reply keeps its expandable trail
/// after a reload. **They were never given back to the butler.**
///
/// Attached to the reply that produced them rather than added as tool results, because a
/// tool result has to answer a call the model made in *this* conversation, and these answer
/// calls from a conversation that has ended. Bounded twice, for the reason every bound here
/// exists: a weak model reads a long context worse than a short one.
#[must_use]
pub fn seeded_from(
    history: &[ChatMessage],
    findings: &dyn Fn(&str) -> Vec<String>,
) -> Vec<TurnMessage> {
    history
        .iter()
        .map(|m| match m.role() {
            MessageRole::User => TurnMessage::User(m.text().to_owned()),
            MessageRole::Butler => {
                let found: Vec<String> = findings(&m.id().value().to_string())
                    .into_iter()
                    .filter(|f| !f.trim().is_empty())
                    .take(MOST_FINDINGS_CARRIED)
                    .map(|f| {
                        let short: String = f.chars().take(LONGEST_FINDING_CARRIED).collect();
                        short
                    })
                    .collect();
                let text = if found.is_empty() {
                    m.text().to_owned()
                } else {
                    format!("{}\n(what I found then: {})", m.text(), found.join(" | "))
                };
                TurnMessage::Assistant {
                    text,
                    tool_calls: Vec::new(),
                }
            }
        })
        .collect()
}

/// The butler's own score, said the way it would say it about anything else.
///
/// The reward half of the loop spine: every number here is derived from read-back the
/// system already records — never from the model's account of itself, which is the thing
/// that can be untrue (ADR 0053). Nothing new is stored; the nightly pass says this into
/// the activity trail, where the person can read it and the butler's own-activity skill
/// can reach it from any turn — so "how have you been doing?" has a grounded answer, and
/// the night's reflection has a measurement to chew on instead of a feeling.
///
/// `None` when there is nothing to score: a scorecard over zero attempts is a vibe.
#[must_use]
pub fn scorecard(outcomes: &[Outcome]) -> Option<String> {
    if outcomes.is_empty() {
        return None;
    }
    let tally = how_each_capability_landed(outcomes);
    let tried: u32 = tally.values().map(|(_, t)| t).sum();
    let confirmed: u32 = tally.values().map(|(c, _)| c).sum();
    let mut said = format!("Scorecard: {confirmed} of {tried} actions confirmed by read-back");
    // The worst habit, named — a tool tried enough to judge and never once seen to work.
    // This is the line a notion can grow from, and the line that once justified a skill
    // withdrawing itself.
    if let Some((worst, (_, tries))) = tally
        .iter()
        .filter(|(_, (c, t))| *c == 0 && *t >= PROVEN_AFTER)
        .max_by_key(|(_, (_, t))| *t)
    {
        said.push_str(&format!("; {worst} has never worked in {tries} tries"));
    }
    // Named, because a tool quietly ceasing to act on its own is exactly the kind of change
    // ADR 0053 says the person hears about rather than notices.
    let taken_back: Vec<String> = {
        let judged = how_they_judged(outcomes);
        let mut ids: Vec<String> = judged
            .iter()
            .filter(|(id, counts)| {
                withdrawn_by_them(**counts)
                    && tally.get(*id).is_some_and(|(c, _)| *c >= PROVEN_AFTER)
            })
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    };
    if let Some(first) = taken_back.first() {
        said.push_str(&format!(
            "; {first} earned acting on its own and you took it back"
        ));
    }
    let graduated = proven_by_the_record(outcomes).len();
    if graduated > 0 {
        said.push_str(&format!(
            "; {graduated} tool{} earned acting without asking",
            if graduated == 1 { " has" } else { "s have" }
        ));
    }
    said.push('.');
    Some(said)
}

/// Where the person said they are based, from what they told Endora.
///
/// Read from the preference they set on first run rather than inferred, because the model
/// reads its own past replies in the same window and a wrong answer it once gave becomes
/// three of the last twelve messages — evidence, as far as a small model is concerned.
/// The person's own words outrank the butler's recollection of itself.
#[must_use]
pub fn where_they_are(preferences: &[Preference]) -> String {
    preferences
        .iter()
        .filter_map(|p| {
            // Read the shape that is actually stored, not the one this expected. The
            // console writes "Based in: <place>" with a colon; a parser matching only
            // "based in " found nothing and the fact silently stayed empty — which is the
            // same mistake as assuming a tool's argument name instead of checking it.
            //
            // So: case-insensitive prefix, then strip whatever punctuation separates the
            // label from the answer.
            let text = p.text().trim();
            let lower = text.to_ascii_lowercase();
            let rest = lower.strip_prefix("based in")?;
            let place = text[text.len() - rest.len()..]
                .trim()
                .trim_start_matches([':', '-', '\u{2014}', '\u{2013}'])
                .trim()
                .trim_end_matches('.')
                .trim();
            (!place.is_empty()).then(|| place.to_owned())
        })
        // The latest one they set wins, the same rule every other preference follows.
        .next_back()
        .unwrap_or_default()
}

/// Marks a tool result as somebody else's words (ADR 0064). Defined at the door it
/// ultimately guards (ADR 0069); re-exported here for the paths that write the mark.
pub use crate::egress::STRANGER_MARK;

/// How many read-back confirmed changes prove a tool (ADR 0062).
///
/// The same arithmetic as a notion maturing: a count in code, which the model cannot argue
/// with and the person does not administer. Three, because one confirmed change could be a
/// coincidence of timing and two a repeat of it; a percentage was rejected because a ratio
/// invites gaming by volume and reads as tunable.
pub const PROVEN_AFTER: u32 = 3;

/// The tools the record has proven: enough outcomes where read-back **saw the world
/// change** (ADR 0062). Only `changed: Some(true)` counts — a claim without read-back is
/// the thing that can be untrue, and an unread effect proves nothing.
///
/// Derived, never stored: delete an outcome and the proof recedes with it, purge
/// everything and every graduate goes back to asking.
#[must_use]
pub fn proven_by_the_record(outcomes: &[Outcome]) -> std::collections::HashSet<String> {
    let judged = how_they_judged(outcomes);
    how_each_capability_landed(outcomes)
        .into_iter()
        .filter(|(_, (confirmed, _))| *confirmed >= PROVEN_AFTER)
        .filter(|(id, _)| !withdrawn_by_them(judged.get(id).copied().unwrap_or((0, 0))))
        .map(|(id, _)| id)
        .collect()
}

/// What the person has said about each capability: `(helped, did not help)`.
///
/// [`Reaction::NoReaction`] is a third answer and not a middle one — *they saw it and it
/// made no difference* is information about the moment, not about the tool, and counting it
/// either way would put a thumb on a scale it does not belong on.
#[must_use]
pub fn how_they_judged(outcomes: &[Outcome]) -> std::collections::HashMap<String, (u32, u32)> {
    let mut tally: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
    for o in outcomes {
        let entry = tally.entry(o.capability().to_owned()).or_insert((0, 0));
        match o.reaction() {
            Some(Reaction::Helped) => entry.0 += 1,
            Some(Reaction::DidNotHelp) => entry.1 += 1,
            _ => {}
        }
    }
    tally
}

/// Whether the person has taken back what the record granted.
///
/// Read-back proves the world changed. It cannot prove the change was **wanted**, and until
/// now nothing in the graduation path could tell those apart: a tool that reliably moved
/// something the person kept saying was unhelpful still earned the right to act without
/// asking, because the only question asked was whether a light went off.
///
/// Their verdict was already collected — the console has the buttons, and repeating an ask
/// marks the actions before it as unhelpful on its own — and it went nowhere except into a
/// sentence in the prompt. That is the mistake [0065](../../docs/adr/0065-a-place-is-not-the-models-to-remember.md)
/// named one layer down: a fact the system holds, narrated to a model instead of deciding
/// anything.
///
/// Symmetric with [`PROVEN_AFTER`] deliberately. Three confirmed changes grant it; three net
/// unhelpful verdicts take it back. One stray mark cannot withdraw a working tool — which
/// matters because the repeat-ask path marks *every* successful action in its window, and
/// that is a coarse instrument to hang a single-strike rule on.
#[must_use]
pub fn withdrawn_by_them((helped, did_not): (u32, u32)) -> bool {
    did_not >= PROVEN_AFTER && did_not > helped
}

/// How each capability has actually landed: `(confirmed changed, times tried)`.
///
/// Only read-back counts. An actuator's own claim is the thing that can be untrue
/// ([0053](../../docs/adr/0053-honesty-about-what-it-did.md)), so `changed: Some(true)` is
/// the numerator and everything else — a false, or an unreadable `None` — is only a try.
#[must_use]
pub fn how_each_capability_landed(
    outcomes: &[Outcome],
) -> std::collections::HashMap<String, (u32, u32)> {
    let mut tally: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
    for o in outcomes {
        let entry = tally.entry(o.capability().to_owned()).or_insert((0, 0));
        entry.1 += 1;
        if o.changed() == Some(true) {
            entry.0 += 1;
        }
    }
    tally
}

/// is grounded in that rather than starting cold each turn.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state — the same reason every other \
              orchestrator here names its stores"
)]
pub fn butler_context(
    beliefs: &impl BeliefRepository,
    // Where they are is a fact the turn is handed, not something to infer from prose it
    // reads beside its own past replies.
    preferences: &impl PreferenceRepository,
    outcomes: &impl OutcomeRepository,
    aliases: &(impl endora_capabilities::TargetAliasRepository + endora_capabilities::ConfigWriteLog),
    chat: &impl ChatRepository,
    capabilities: &dyn CapabilityRunner,
    transitions: &impl endora_capabilities::TransitionLog,
    clock: &impl Clock,
) -> Result<ButlerContext, AppError> {
    let understanding = understanding(beliefs, clock)?
        .into_iter()
        .map(|b| {
            format!(
                "[{}] {} ({} confidence)",
                b.kind().name(),
                b.statement(),
                b.confidence().name()
            )
        })
        .collect();
    // Ground the butler in the skills it can actually reach right now (configured
    // ones only), so it uses a real capability instead of only talking about it.
    let available: Vec<_> = capabilities
        .available()
        .into_iter()
        .filter(|c| c.configured)
        .collect();
    // How its own past actions landed (ADR 0053) — a bounded read, since only the
    // recent stretch is informative and the prompt has to stay small. It now pays twice:
    // the same read-back that keeps the butler honest also says which tools deserve to be
    // reached for first (ADR 0060).
    let recent = recent_outcomes(outcomes, TRACK_RECORD_WINDOW)?;
    // Reads stay in front of the turn; actuators wait behind one lookup (ADR 0060).
    let (offered, deferred) = offered_and_deferred(available, &how_each_capability_landed(&recent));
    let skills = offered
        .iter()
        .map(|c| format!("{} — {}", c.id, c.description))
        .collect();
    // The same skills structured for native tool-calling (exact id + input schema).
    let mut tools: Vec<CapabilityTool> = offered
        .iter()
        .map(|c| CapabilityTool {
            id: c.id.clone(),
            description: c.description.clone(),
            input_schema: c.input_schema.clone(),
        })
        .collect();
    // The way back. Without this the deferral is a deletion, and a deletion the model
    // cannot see is the failure this whole record exists to avoid: it apologises for tools
    // it was never shown instead of asking for them.
    if !deferred.is_empty() {
        tools.push(CapabilityTool {
            id: LOOK_FOR_A_TOOL.to_owned(),
            description: "Find more tools. Only some are listed above; the rest — acting on \
                          things, and further ways of looking things up — are behind this. \
                          Call it whenever the tools you can see do not cover what was asked, \
                          and say what is wanted in the person's own words."
                .to_owned(),
            input_schema: Some(
                r#"{"type":"object","properties":{"what_they_want":{"type":"string"}},"required":["what_they_want"]}"#
                    .to_owned(),
            ),
        });
    }
    let deferred = deferred
        .into_iter()
        .map(|c| CapabilityTool {
            id: c.id,
            description: c.description,
            input_schema: c.input_schema,
        })
        .collect();
    Ok(ButlerContext {
        understanding,
        where_they_are: where_they_are(&preferences.list_all()?),
        deferred,
        capabilities: skills,
        tools,
        // Live, and cheap: the services already read for other reasons know whether the
        // person is in the house.
        //
        // Plus **what has changed lately, from every source that can say what it knows**.
        //
        // This is the seam that makes adding an integration free. A source used to reach the
        // turn only by implementing `about_the_person` and phrasing itself — which exactly one
        // integration ever did, so everything else was reachable in principle and silent in
        // practice, waiting for a weak model to think of calling it. The morning brief taught
        // that lesson once already and it was never generalised.
        //
        // Changes rather than an inventory, for the reason the budget exists: "the back door
        // opened ten minutes ago" is worth a turn and "light.hall is off" is not. It is
        // bounded by what actually moved, which is why it can be automatic instead of
        // something the person has to nominate per source.
        present: capabilities
            .about_the_person()
            .into_iter()
            .chain(what_changed_lately(transitions, clock.now().unix_millis()))
            .collect(),
        // What Endora itself has been doing, stated rather than fetched (ADR 0056). The
        // skill version of this was shipped first and a 7B model answered "did you do
        // anything while I was out?" by trying to turn a light on.
        did_lately: recently_did(chat, outcomes, aliases, clock)?,
        now: clock_line(clock),
        conversation_summary: None,
        track_record: track_record(&recent),
        // Set by whoever holds the person's "new topic" choice; the context itself does not know it.
        context_from_ms: 0,
        // What the person has said these targets are really called, and what Endora has
        // since made to stand for several of them (ADR 0054).
        target_aliases: aliases
            .aliases()?
            .iter()
            .map(endora_capabilities::TargetAlias::as_context)
            .chain(collections_worth_naming(aliases))
            .collect(),
    })
}

/// The last few things Endora did, short enough to sit in every turn's context.
///
/// The same facts [`what_it_has_been_doing`] reports, trimmed to what fits: the most recent
/// handful, newest last. A turn pays for its whole context, so this is a summary and the
/// skill is the detail.
pub(super) fn recently_did(
    chat: &impl ChatRepository,
    outcomes: &impl OutcomeRepository,
    writes: &impl endora_capabilities::ConfigWriteLog,
    clock: &impl Clock,
) -> Result<Vec<String>, AppError> {
    /// Enough to answer "did anything happen?" without becoming the prompt.
    const FITS_IN_A_TURN: usize = 4;
    let now_ms = clock.now().unix_millis();
    let report = what_it_has_been_doing(chat, outcomes, writes, now_ms - A_DAY_MS, now_ms)?;
    // The empty answer is a sentence, not a list, and belongs here as one line: "nothing
    // happened" is exactly what the butler needs to be able to say without guessing.
    Ok(report
        .lines()
        .rev()
        .take(FITS_IN_A_TURN)
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect())
}

/// The facts a written brief has **not** already said, so nothing is printed twice.
///
/// Compared on **words, not phrasing**. An exact-substring check was tried and failed on the
/// first live brief: the facts said *"john is not home; on the Family calendar: Yardwork"*
/// and the model wrote *"john is not home right now. On the Family calendar, there's
/// Yardwork"* — the same thing, and not the same string. Any check against model output that
/// depends on wording is a check that will fail.
///
/// Content words only, and **all** of them must appear. The bias is deliberate: a fact
/// wrongly thought missing is printed twice, which is untidy, where one wrongly thought
/// covered is simply lost — and the whole reason the facts are appended is that losing them
/// is what kept happening.
pub(super) fn not_yet_said(written: &str, facts: &str) -> Vec<String> {
    let said = words_of(written);
    facts
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|line| {
            let fact = line.trim().trim_start_matches("- ");
            let content: Vec<String> = words_of(fact).into_iter().filter(|w| w.len() > 3).collect();
            // Nothing distinctive to match on: keep it rather than guess.
            content.is_empty() || !content.iter().all(|w| said.contains(w))
        })
        .map(str::to_owned)
        .collect()
}

/// Asks a stronger model to word a brief from facts that have already been gathered.
///
/// `None` when it cannot be reached or says nothing usable, and the caller falls back — a
/// brief must never depend on somebody else's service being up.
///
/// What leaves is the facts and nothing else. The context is **empty on purpose**: the deep
/// model is being asked to write, not to reason about the person, so their beliefs, the
/// names they use for things and how their skills have been landing have no reason to travel
/// (ADR 0051). And what does travel is disguised, so the values are placeholders and are put
/// back here.
pub(super) fn word_the_brief(
    deeper: &crate::egress::Deeper,
    facts: &str,
    ids: &impl IdSource,
    clock: &impl Clock,
) -> Option<crate::egress::Worded> {
    // The instruction is Endora's own sentence; the facts travel through the door, which
    // disguises them, scans what would leave, and restores the reply (ADR 0069).
    deeper.word(
        "Write my morning brief from exactly these facts, using all of them. Keep it \
         short and warm, do not invent anything, and leave any placeholder in angle \
         brackets exactly as it is:",
        facts,
        ids,
        clock,
    )
}

/// How far back the turn is told about, and how much of it.
///
/// Small on both counts. The turn is not a log — it is what somebody would mention walking
/// into a room, and a butler that opened every conversation with forty state changes would
/// have made the context worse for a weak model rather than better.
pub(super) const CHANGES_WORTH_MENTIONING_MS: i64 = 6 * 60 * 60 * 1_000;
pub(super) const MOST_CHANGES_IN_A_TURN: usize = 6;

/// What has moved lately, in the turn's own voice.
///
/// **The seam that makes a new integration free.** Anything that can say what it currently
/// knows is watched, and anything watched reaches the turn when it changes — without the
/// person nominating it and without a line of wiring per source.
///
/// Most recent first and capped, because the budget is the whole reason this can be automatic:
/// an inventory would have to be opted into, a short list of what actually changed does not.
#[must_use]
pub fn what_changed_lately(
    transitions: &impl endora_capabilities::TransitionLog,
    now_ms: i64,
) -> Vec<String> {
    let mut moved = transitions
        .since(now_ms - CHANGES_WORTH_MENTIONING_MS)
        .unwrap_or_default();
    moved.sort_by_key(|t| std::cmp::Reverse(t.at_ms));
    moved
        .into_iter()
        .take(MOST_CHANGES_IN_A_TURN)
        // The name carries its source already, so this says where a fact came from without
        // having to be told.
        .map(|t| format!("{} changed from {} to {}", t.key, t.from, t.to))
        .collect()
}

/// Whether a `context.present` line is one of [`what_changed_lately`]'s raw
/// transitions rather than a curated, human-authored fact. The exact phrase that
/// function builds — see the shared shape between the two — is the only thing
/// checked for, deliberately: it is a description of the one producer, not a guess
/// at what "looks technical."
pub(super) fn is_a_raw_transition(line: &str) -> bool {
    line.contains(" changed from ") && line.contains(" to ")
}

/// The facts a morning brief is *for*, gathered from what Endora already holds.
///
/// The brief was one instruction — *"reach for whatever's relevant"* — and four days of them
/// read like "the kitchen and garage lights are on". Every fact worth having was already in
/// the turn: what is on today, what it is like outside, what Endora did overnight. (Not what
/// has stopped answering: the owner turned that off, 2026-10-16.) None of it depended on the model choosing a tool, and all of it was
/// left to the model to choose a tool for.
///
/// Assembled deterministically for the same reason the activity account is
/// ([0056](../../docs/adr/0056-how-it-behaves-toward-you.md)): a report of stored facts is
/// the one thing Endora is entitled to assert, and the model's job is to say it nicely.
pub(super) fn whats_worth_saying_this_morning(context: &ButlerContext) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    // One fact per line. The services join theirs with "; ", which reads as a run-on in a
    // brief and — more importantly — makes coverage all-or-nothing: a written brief that
    // mentions two of three things could not be credited with either.
    //
    // `context.present` is shared with the chat turn, where a raw transition ("the back
    // door opened ten minutes ago") is exactly the grounding a reply wants. A brief is
    // read once, cover to cover, and the same feed's untranslated entries —
    // `home-assistant::sensor.bambam_pressure changed from 14.484… to 14.485…` — are not
    // a fact a person asked to start their day with; live, a page of them is what made a
    // brief unreadable. Filtered by shape (`is_a_raw_transition`), not by source: nothing
    // the curated `about_the_person` lines say is ever shaped that way.
    lines.extend(
        context
            .present
            .iter()
            .filter(|line| !is_a_raw_transition(line))
            .flat_map(|line| line.split("; "))
            .map(str::to_owned),
    );
    // Deliberately NOT what Endora has been doing. That answers "did you do anything while
    // I was out?", which is a different question — and put in a brief it reads as Endora
    // quoting its own message log back, including the messages that went wrong:
    //
    //   10:47 — I wrote to you unprompted: "Sure, please provide me with a function name…"
    //
    // A morning brief is about the person's day, not the butler's.
    // `None` rather than "nothing to report": with no facts AND no model, there is no
    // brief at all. Posting a cheerful nothing would be Endora claiming to have thought
    // about the day, which is the floor ADR 0053 refuses to build.
    (!lines.is_empty()).then(|| {
        lines
            .iter()
            .map(|l| format!("- {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// The collections Endora has made, as lines the butler can act on.
///
/// A group is useless if the model does not know it exists. Live: "All Lights" was created
/// and worked perfectly when named — and "turn off all lights" still produced a call aimed
/// at nothing, because nothing told the model there was now one thing to name.
///
/// This is grounding, exactly as a confirmed alias is: what a thing is called here. Only
/// collections Endora made and that have not been undone, so the list stays short and
/// every line is something that really exists.
pub(super) fn collections_worth_naming(
    log: &impl endora_capabilities::ConfigWriteLog,
) -> Vec<String> {
    log.writes(COLLECTIONS_SHOWN)
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.kind == endora_capabilities::WriteKind::Collection && !w.undone)
        .map(|w| {
            format!(
                "on {}, \"{}\" is one thing standing for {} others — name it to act on \
                 them all at once",
                w.server,
                w.added,
                w.was.len()
            )
        })
        .collect()
}

/// How far back to look for collections. They are rare and long-lived; this only bounds
/// the read.
pub(super) const COLLECTIONS_SHOWN: usize = 50;

/// Records a preference the butler should keep in mind. In this build every
/// preference is created by explicit confirmation, so it is always a *stated*
/// preference (the person's own words), never inferred.
///
/// # Errors
/// [`AppError::Domain`] if the text is blank, or [`AppError::Repository`] on
/// failure.
pub fn create_preference(
    preferences: &impl PreferenceRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    text: &str,
    kind: PreferenceKind,
) -> Result<Preference, AppError> {
    let preference = Preference::new(PreferenceId::new(ids.new_id()), text, kind, clock.now())?;
    preferences.save(&preference)?;
    // Handed back as the store keeps it: a store may name a line by its words (the brain
    // does, ADR 0101), and the id the caller deletes it by must be that one.
    let said = |p: &Preference| p.text().split_whitespace().eq(text.split_whitespace());
    Ok(preferences
        .list_all()?
        .into_iter()
        .find(said)
        .unwrap_or(preference))
}

/// Lists the preferences the butler has learned, oldest first.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn list_preferences(
    preferences: &impl PreferenceRepository,
) -> Result<Vec<Preference>, AppError> {
    Ok(preferences.list_all()?)
}

/// Forgets a preference (memory is correctable and deletable).
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn delete_preference(
    preferences: &impl PreferenceRepository,
    id: PreferenceId,
) -> Result<(), AppError> {
    preferences.delete(id)?;
    Ok(())
}
