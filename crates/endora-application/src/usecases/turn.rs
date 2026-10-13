use super::*;

/// Marks a tool result as **evidence** or as an unverified **receipt** (ADR 0053).
///
/// Endora's architecture principle is *models propose, policy authorizes,
/// capabilities execute, **evidence verifies***. The first three were built; the
/// fourth was not, and the turn simply believed whatever an actuator claimed. That
/// gap is not hypothetical: Home Assistant was asked to turn a light off with a tool
/// that only sets brightness, matched the targets, changed nothing, and answered
/// `action_done` — so the butler announced success while the light stayed on. No
/// amount of grounding helps a model whose tool told it everything went fine.
///
/// A capability in the [`Reversibility::Observe`] band *reports state*, so its result
/// is an observation and stands on its own. Anything else returns the actuator's
/// account of its own work, which is exactly the thing that can be wrong. Saying so
/// in the tool result is not narration on the butler's behalf (ADR 0053) — it is part
/// of the real outcome, and it is the honest default for every integration, including
/// ones nobody has debugged yet.
pub fn note_verification(
    output: &str,
    spec: Option<&crate::CapabilitySpec>,
    observation: Option<&str>,
) -> String {
    note_verification_against(output, spec, None, observation)
}

/// As [`note_verification`], but knowing what the world looked like **before**.
///
/// Only the nominated reader of a server is classified as observing; every other tool on
/// it is treated as an actuator, because the server says nothing trustworthy about which
/// is which and deny-by-default is right for *authorization* (ADR 0054). Reusing that
/// classification for *verification* is where it goes wrong. Observed live: a call asking
/// Home Assistant for the **time** came back with five kilobytes of every device in the
/// house attached, under the instruction "answer from the OBSERVATION". The model was told
/// to answer "what time is it" from a list of lamps, three times in one turn, and the
/// morning briefing it eventually produced was one sentence about a bedroom light.
///
/// The fix is not to guess which tools actuate. It is that **an observation showing nothing
/// moved has nothing to show**: [`note_unchanged`] already states that verdict in one
/// sentence, which is the sharp signal [0053](../../docs/adr/0053-honesty-about-what-it-did.md)
/// actually cares about. Pasting the reading as well adds no information, floods the
/// context, and misdirects the answer.
///
/// So the reading travels when it differs from what was there before — when something
/// genuinely moved and the detail is the evidence — and not otherwise. Nothing about the
/// honesty guarantee changes: every call is still read back, and every verdict is still
/// reported.
pub(super) fn note_verification_against(
    output: &str,
    spec: Option<&crate::CapabilitySpec>,
    before: Option<&str>,
    observation: Option<&str>,
) -> String {
    let observes = spec.is_some_and(|s| s.reversibility == Reversibility::Observe);
    if observes {
        return output.to_owned();
    }
    let Some(observed) = observation else {
        return format!(
            "{output}\n\n[unverified] This is what the tool reported about its own work. \
             Endora has NOT independently confirmed the effect. Say what was reported and \
             that it is unconfirmed — do not state the world has changed as though you had \
             checked."
        );
    };
    if before.is_some_and(|b| b.trim() == observed.trim()) {
        // Identical to what was there before, so the reading is not evidence of anything
        // this call did. `note_unchanged` supplies the verdict; repeating the whole world
        // here would only bury it.
        return format!(
            "{output}\n\n[observed] Endora read the state back and it is unchanged from \
             before the call."
        );
    }
    format!(
        "{output}\n\n[observed] Endora then read the state back. This is what the world \
         actually looks like now:\n{observed}\n\nAnswer from the OBSERVATION, not from \
         what the tool claimed. If they disagree, the observation wins and you should say \
         the action did not take effect."
    )
}

/// Says so when the thing an action was aimed at **is not answering** (ADR 0056).
///
/// Endora knows this and has never said it. Live: "turn on the guest bedroom left lamp"
/// failed, and `Guest Bedroom Left` had been unavailable for days — the service could not
/// reach it, so nothing was ever going to happen. The person got a failure with no cause,
/// and the cause was sitting in the reading Endora had just taken.
///
/// Read from the **live** state rather than from the standing-trouble record, because a
/// device that went quiet an hour ago is exactly as unreachable as one that went quiet on
/// Tuesday, and waiting three days to mention it helps nobody mid-request.
///
/// Names are matched longest-first for the same reason the facts disclosure does it: a call
/// aimed at `Kitchen Main Light` must not be explained by something called `Kitchen`.
pub(super) fn note_not_answering(named_in: &str, mut states: Vec<(String, String)>) -> String {
    let asked = words_of(named_in);
    states.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    if let Some((name, state)) = states.iter().find(|(name, state)| {
        endora_capabilities::not_answering(state) && contains_all_words(&asked, &words_of(name))
    }) {
        return format!(
            "\n\n[not answering] {name} is `{state}` — the service cannot reach it at all, so \
             nothing was going to happen here whatever the tool reported. Say that plainly: it \
             is the device, not the request."
        );
    }
    // Nothing named one thing — but "check the lights" names a KIND, and the kind is
    // the first word of the service's own key (`light.kitchen_table`), never an English
    // list; a service whose keys carry no kind contributes nothing. A trailing `s` is
    // trimmed because people speak of their lights and a service names a light.
    //
    // Live, 2026-08-06: "tell me if any are offline". The answer was sitting in the
    // reading, no single name was in the ask, and the model — which follows an
    // instruction about verification one run in three — invented a recap instead.
    // This line is the deterministic floor under that question: whatever the prose
    // says, the dead things in the house are named beside it (append, never rewrite —
    // ADR 0056).
    const MOST_NAMED: usize = 5;
    let asked_kinds: Vec<String> = asked
        .iter()
        .map(|w| w.strip_suffix('s').unwrap_or(w).to_owned())
        .collect();
    let mut dead: Vec<String> = states
        .iter()
        .filter(|(name, state)| {
            endora_capabilities::not_answering(state)
                && words_of(name)
                    .first()
                    .is_some_and(|kind| asked_kinds.contains(kind))
        })
        .map(|(name, state)| format!("{name} is `{state}`"))
        .collect();
    if dead.is_empty() {
        return String::new();
    }
    if dead.len() == 1 {
        return format!(
            "\n\n[not answering] {} — the service cannot reach it at all. Say that \
             plainly: it is the device, not the request.",
            dead.remove(0)
        );
    }
    let more = dead.len().saturating_sub(MOST_NAMED);
    dead.truncate(MOST_NAMED);
    let mut list = dead.join(" · ");
    if more > 0 {
        list.push_str(&format!(" · and {more} more"));
    }
    format!(
        "\n\n[not answering] {list} — the service cannot reach these at all. Say that \
         plainly: it is the devices, not the request."
    )
}

/// Says so when the world is **identical before and after** an action.
///
/// The sharpest form of the failure ADR 0053 exists for. Asked to turn the kitchen
/// light off, Home Assistant reported *"completed successfully on: Kitchen (area),
/// Kitchen Table (light)"* — entirely true, and useless: `Kitchen Table` was
/// unavailable and `Kitchen Main`, the switch that was actually on, stayed on. A claim
/// can be accurate about what it touched and still change nothing.
///
/// Two readings settle it without interpreting either. This compares strings and knows
/// nothing about any server's format, so it holds for every integration — the
/// observation-derived fact ADR 0054 named, and the one that makes the loop accumulate
/// rather than needing a patch per quirk.
///
/// Silent unless both readings exist and match: without a before, or with no reader at
/// all, there is nothing to claim and the honest answer is to say nothing.
pub(super) fn did_change(before: Option<&str>, after: Option<&str>) -> Option<bool> {
    let (before, after) = (before?, after?);
    Some(before.trim() != after.trim())
}

/// How long to let a service settle before looking a second time.
///
/// Long enough for a smart-home integration to report a state change back (they are
/// typically well under half a second on a local network), short enough that a person
/// waiting on a reply does not notice. Paid only when the first reading suggested nothing
/// happened — which is either a real no-op, where the wait is the cost of being sure, or a
/// race, where it is the difference between the truth and a false record.
pub(super) const SETTLE_BEFORE_LOOKING_AGAIN: std::time::Duration =
    std::time::Duration::from_millis(700);

/// The tool-result note when the world **did** move.
///
/// `note_unchanged` has always spoken up when nothing happened, and said nothing when
/// something did — which leaves the model with only the service's own sentence to go on.
/// Live, that sentence is often a hedge ("Home Assistant accepted this; its reply says
/// nothing about the result"), because Home Assistant answers before its integrations
/// report back. Endora had already compared two readings and knew the answer; it just was
/// not saying it.
///
/// Stating the verdict is not narration on the model's behalf (ADR 0053) — it is reporting
/// an observation Endora made, which is the one thing it is entitled to assert.
pub(super) fn note_changed(before: Option<&str>, after: Option<&str>) -> String {
    let (Some(before), Some(after)) = (before, after) else {
        return String::new();
    };
    if before.trim() == after.trim() {
        return String::new();
    }
    "\n\n[changed] Endora read the state before and after, and it is different — this \
     worked. Say so plainly; do not hedge about it."
        .to_owned()
}

/// The tool-result note for [`did_change`] — see it for why two readings settle this.
pub(super) fn note_unchanged(before: Option<&str>, after: Option<&str>) -> String {
    let (Some(before), Some(after)) = (before, after) else {
        return String::new();
    };
    if before.trim() != after.trim() {
        return String::new();
    }
    "\n\n[unchanged] Endora read the state before and after, and it is identical — \
     whatever the tool reported, nothing actually changed. Say that plainly, and if \
     something in the reading looks like what they meant, name it and ask."
        .to_owned()
}

/// Reads the world back after an actuation, so the turn answers from what is
/// **observed** rather than from what the actuator claimed (ADR 0053).
///
/// Runs after a failure too, and deliberately: a failed action's most useful output
/// is what actually exists. The live failure this was built for — `HassTurnOff`
/// returning `no_match_reason=AREA` — is far more actionable once the reply also
/// carries the entities that *are* in that area.
///
/// Best-effort: if the read fails there is simply no observation, and the result
/// falls back to being marked unverified. Verification must never turn a working
/// action into a broken turn.
pub(super) fn read_state_back(
    capabilities: &dyn CapabilityRunner,
    id: &str,
    action_input: &str,
) -> Option<String> {
    let verifier = capabilities.verifier(id)?;
    let spec = capabilities
        .available()
        .into_iter()
        .find(|c| c.id == verifier)?;
    if !spec.configured {
        return None;
    }
    // Narrow the reading to what the action was aimed at (ADR 0053). The capabilities
    // context owns schema knowledge, so it works out which of the action's targeting
    // arguments this reader also accepts.
    let input = capabilities.read_back_input(id, action_input);
    capabilities.run(&verifier, &input).ok()
}

/// The single tool-calling conversation (ADR 0053). Seeds the conversation from the
/// chat history, then drives [`Butler::take_turn`]: each tool call the model makes is
/// executed **through policy**, and its real result — success **or** error — is
/// appended as a [`TurnMessage::ToolResult`] the model answers from in-context, until
/// it replies with no tool call. Bounded by `max_rounds` and a per-turn failure cap.
///
/// The loop adds **no canned narration**: an un-runnable tool yields a factual tool
/// result (needs setup / needs confirmation / failed), and the model writes the
/// user-facing answer from it. If the model misreports with the truth in front of it,
/// that is a model failure to surface — not a string to hardcode (ADR 0053).
/// Ask the model for one turn, retrying a completely empty completion.
///
/// A slow local model occasionally returns *nothing* — no tool call and no text —
/// on a turn it should have acted on (observed ~1 in 3 on qwen2.5:14b). Left alone
/// that whiff surfaces as the canned "not sure how to help" fallback with an empty
/// activity log. Retrying is cheap (an empty completion returns near-instantly) and,
/// because sampling is non-deterministic, usually turns the retry into a real tool
/// call or answer. Bounded so a persistently mute model still returns promptly.
/// Whether a reply is not an answer at all — the model responding to its own plumbing
/// rather than to the person.
///
/// Two shapes, one meaning. Both were observed within an hour of each other:
///
/// ```text
/// "here are the appropriate function calls: 1. **GetWeather** ..."   (named, called none)
/// "None of the functions provided pertain to the 'news' domain."     (protocol words)
/// ```
///
/// Treating this as **no reply** rather than as a bad one is what keeps it to a single
/// idea: every path already knows what to do when the model says nothing. The turn
/// retries; a check-in stays quiet; a chat answer falls back. None of them needed a new
/// mechanism, only a truer notion of "nothing".
pub(super) fn not_an_answer(reply: &ButlerReply, context: &ButlerContext) -> bool {
    reply.tool_calls.is_empty()
        && (reply.text.trim().is_empty()
            // The model was unreachable, so there is no answer here to keep — only an
            // apology for its absence. Retried where someone is waiting; silent where
            // nobody asked (ADR 0056).
            || reply.degraded
            || only_described_a_tool(reply, context)
            || sounds_like_plumbing(&reply.text))
}

/// Whether a message is about Endora's plumbing rather than about the person.
///
/// A deliberate, narrow heuristic — and named as one. Two kinds of phrase, both of which
/// mean the reply is about the machinery rather than about the person: words from the tool
/// protocol, and the model disclaiming its own nature.
///
/// The consequence differs by path, and neither loses an answer:
///
/// - **unprompted** (a check-in, a brief) — suppressed, because silence is already the
///   default there and a false positive costs one skipped message;
/// - **asked for** — *retried*, bounded, and if the retries run out the person sees the
///   reply anyway. Suppressing an answer someone asked for would be worse than an awkward
///   one, so nothing is ever withheld.
pub(super) fn sounds_like_plumbing(text: &str) -> bool {
    const PLUMBING: &[&str] = &[
        "function call",
        "functions provided",
        "exposed entities",
        "no exposed",
        "json object",
        "placeholder argument",
        "the 'news' domain",
        "tool call",
        // Bare and singular, because a butler has no reason to say the word at all. Two
        // live misses, each by less than a word: the list held "functions provided" and
        // missed "None of the functions listed involve a 'person' domain"; then it held
        // "functions" and missed "please provide me with a function name". An exact-phrase
        // list will always fail this way, so the marker is the shortest thing that can
        // only be plumbing.
        "function",
        // Breaking the frame is the same failure wearing different clothes: the reply is
        // about what the model is rather than about what was asked. Live, to "How has your
        // day been": "I'm an AI assistant without personal experiences, so I don't have a
        // day to reflect on." A butler with a persona floor (ADR 0056) does not answer a
        // pleasantry by disclaiming its own nature, and asking the prompt to prevent it is
        // the kind of guarantee this architecture keeps in code instead.
        "i'm an ai assistant",
        "i am an ai assistant",
        "as an ai language model",
        "as an ai assistant",
        "i don't have personal experiences",
        "without personal experiences",
        "i do not have feelings",
    ];
    let lowered = text.to_lowercase();
    PLUMBING.iter().any(|marker| lowered.contains(marker))
}

/// Whether the model wrote *about* the tools instead of using one.
///
/// Observed, in answer to "I usually want news weather traffic":
///
/// ```text
/// Based on your request ... here are the appropriate function calls:
/// 1. **GetWeather** - To fetch the current weather.
/// 2. **GetTraffic** - To get traffic updates.
/// Here are the JSON objects for these functions with placeholder arguments:
/// ```
///
/// Nothing ran. A weak model asked to answer with tools available sometimes narrates the
/// call it would make rather than making it, and the person gets an essay about function
/// names instead of their weather.
///
/// The signal is taken from the **catalogue offered this turn**, not from a list of
/// suspicious words: naming a tool it was handed, while calling none of them, is
/// describing rather than doing. A reply that mentions no offered tool is left alone
/// however it is phrased.
pub(super) fn only_described_a_tool(reply: &ButlerReply, context: &ButlerContext) -> bool {
    if !reply.tool_calls.is_empty() {
        return false;
    }
    let text = reply.text.to_lowercase();
    context.tools.iter().any(|tool| {
        // The bare name, since the model writes `GetWeather` rather than
        // `home-assistant.GetWeather`.
        let bare = tool
            .id
            .rsplit('.')
            .next()
            .unwrap_or(&tool.id)
            .to_lowercase();
        bare.len() > 3 && text.contains(&bare)
    })
}

pub(super) fn take_turn_retrying_empty(
    butler: &dyn Butler,
    conversation: &[TurnMessage],
    prefs: &[Preference],
    context: &ButlerContext,
    on_token: &mut dyn FnMut(&str),
    // The turn's trail — so a rung that fires and fails is on the record, not invisible.
    notes: &mut Vec<String>,
) -> Result<ButlerReply, AppError> {
    const MAX_EMPTY_RETRIES: usize = 2;
    let mut reply = butler
        .take_turn_streaming(conversation, prefs, context, on_token)
        .map_err(|e| AppError::Model {
            message: e.to_string(),
        })?;
    let mut retries = 0;
    while let Some(why) = why_nothing_useful(&reply, conversation, context) {
        if retries >= MAX_EMPTY_RETRIES {
            break;
        }
        retries += 1;
        eprintln!("turn: round rejected ({why}) — retry {retries}/{MAX_EMPTY_RETRIES}");
        // A retry only happens when the round produced NOTHING, so nothing was streamed
        // and there is nothing for the person to see rewritten.
        reply = butler
            .take_turn_streaming(conversation, prefs, context, on_token)
            .map_err(|e| AppError::Model {
                message: e.to_string(),
            })?;
    }
    // The local model has now failed the same deterministic check three times. Asking it a
    // fourth time is the definition of not learning, so ask a different one — once
    // (ADR 0055).
    //
    // Reliability compounds: two independent attempts at p₁ and p₂ fail together only at
    // (1-p₁)(1-p₂), which is how a second model beats a bigger one without waiting for a
    // bigger one to exist. What makes this safe rather than clever is that the **trigger is
    // code**: a check Endora applied to the reply, never the model's own opinion of how it
    // did.
    // Through the door, or not at all (ADR 0069): the disguise, the outbound secret scan
    // and the taint refusal (ADR 0064) live inside `Deeper`, so this site decides only
    // *whether* escalation is wanted — never what may leave. The taint refusal lives in
    // the door, derived from the marks each time (ADR 0070) — nothing here to forget.
    if let Some(why) = why_nothing_useful(&reply, conversation, context) {
        eprintln!("turn: escalating to the deep model ({why})");
    }
    if let Some(deeper) = butler
        .deeper()
        .filter(|_| gave_nothing_useful(&reply, conversation, context))
    {
        // Not streamed. The person has already seen whatever the local attempts emitted, and
        // a second voice writing into the same bubble would read as one confused answer.
        //
        // Every branch below reaches the trail, because a rung that fails invisibly is
        // indistinguishable from one that never fired — a live turn gave up after two
        // failed actions and answered with the weather, and the record could not say
        // whether the deep model was refused, unreachable, or never consulted. Now it
        // says (ADR 0064's "and says so", finally kept).
        match deeper.continue_turn(conversation, prefs, context) {
            Ok(better) => {
                if !gave_nothing_useful(&better, conversation, context) {
                    return Ok(better);
                }
                notes.push("Asked the deep model too, and it did no better".to_owned());
            }
            Err(crate::egress::DoorRefusal::AStrangerSpoke) => {
                notes.push(
                    "Couldn't ask the deep model: this turn read something written by \
                     someone else, and that never leaves the box"
                        .to_owned(),
                );
            }
            Err(crate::egress::DoorRefusal::LooksLikeASecret) => {
                notes.push(
                    "Couldn't ask the deep model: the conversation looks like it \
                     contains a secret, and that never leaves the box"
                        .to_owned(),
                );
            }
            Err(crate::egress::DoorRefusal::TheModelFailed(why)) => {
                // The short truth for the person; the full error for the operator.
                eprintln!("the deep model failed: {why}");
                notes.push("Tried the deep model too, but it couldn't answer".to_owned());
            }
        }
    }
    // Every attempt has now produced something the code has determined is not an answer, and
    // there is no deeper model left to ask. Handing the person the raw text is the remaining
    // option and it is the wrong one: what they get is the model narrating its own function
    // list at them.
    //
    // The reasoning above this used to be "suppressing an answer someone asked for would be
    // worse than an awkward one, so nothing is ever withheld", which is right about
    // suppression and wrong about the choice — there is a third option. **Say so.** Not
    // silence, and not the machinery.
    //
    // Live: asked "any emails today", the reply was three paragraphs about which functions
    // pertain to the `email` domain and how Home Assistant integrations are typically wired.
    // Nothing in that is for the person, and there is no reading of it where they are better
    // off having seen it.
    //
    // Marked degraded, which carries the consequence already established (ADR 0056): the
    // person waiting on an answer reads this, and a check-in nobody asked for stays silent.
    // **Only when there is text and the text is machinery.** An empty reply already has a
    // considered path — the caller retries where somebody is waiting and stays silent where
    // nobody asked — and replacing that here would override a decision made somewhere better
    // informed. This is narrowly the case with no good path: words that reached the person
    // and should not have.
    let is_plumbing = !reply.text.trim().is_empty()
        && sounds_like_plumbing(&reply.text)
        && reply.tool_calls.is_empty();
    if is_plumbing {
        return Ok(ButlerReply {
            text: "I haven't got a real answer to that one. I'd rather say so than pad it out."
                .to_owned(),
            degraded: true,
            ..ButlerReply::default()
        });
    }
    Ok(reply)
}

/// How long after an action a repeated request still counts as being about it.
///
/// Long enough to cover reading the reply, looking at the light and asking again; short
/// enough that saying the same thing tomorrow evening is a new request rather than a verdict
/// on yesterday's.
pub(super) const STILL_THE_SAME_ASK_MS: i64 = 10 * 60 * 1000;

/// Marks what a person's **repeated request** says about the action before it (ADR 0052).
///
/// The reaction machinery has existed for months and has taken **zero** inputs across 116
/// outcomes. It was asked for on a settings screen nobody opened, then in the chat where the
/// person already is, and neither produced one. At some point that stops being a placement
/// problem and becomes evidence that asking is the wrong instrument.
///
/// The signal was in the transcript the whole time. **Somebody who asks again has told you it
/// did not work** — no button, no screen, nothing to remember.
///
/// **One direction only.** A repeat is evidence of failure; silence is *not* evidence of
/// success. Somebody who says nothing may be satisfied, or may have given up and gone to the
/// switch, and there is no way to tell them apart. So this derives `DidNotHelp` and never
/// `Helped` — the same asymmetry that lets a refusal support withdrawing a tool while no
/// number of quiet successes supports keeping one
/// ([0054](../../docs/adr/0054-other-peoples-services.md)).
///
/// Only outcomes that **claimed success** are marked. One that already errored is visible as
/// a failure without help from anybody, and recording a reaction on it would double-count.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
pub(super) fn note_what_the_repeat_says(
    outcomes: &impl OutcomeRepository,
    history: &[ChatMessage],
    asking_again: &str,
    now: Timestamp,
) -> Result<usize, AppError> {
    let Some(asked_before) = history.iter().rev().find(|m| m.role() == MessageRole::User) else {
        return Ok(0);
    };
    let since = asked_before.at().unix_millis();
    if now.unix_millis() - since > STILL_THE_SAME_ASK_MS {
        return Ok(0);
    }
    if !says_the_same_thing(asked_before.text(), asking_again) {
        return Ok(0);
    }
    let mut marked = 0;
    for mut outcome in outcomes.list()? {
        let after_the_ask = outcome.at().unix_millis() >= since;
        let claimed_success = !outcome.claim().trim_start().starts_with("error:");
        if after_the_ask && claimed_success && outcome.reaction().is_none() {
            outcome.react(Reaction::DidNotHelp);
            outcomes.save(&outcome)?;
            marked += 1;
        }
    }
    Ok(marked)
}

/// Whether a reply is worth keeping — [`not_an_answer`], plus saying the same thing again.
///
/// Repetition needs the conversation, which is why it lives here rather than in
/// `not_an_answer`. Observed live: asked what it had done while the person was out, the
/// butler reproduced **word for word** the answer it had given the previous day, with the
/// day's real record sitting in its context. A reply that only repeats the last one has
/// added nothing, and asking again is free where the person is waiting.
pub(super) fn gave_nothing_useful(
    reply: &ButlerReply,
    conversation: &[TurnMessage],
    context: &ButlerContext,
) -> bool {
    why_nothing_useful(reply, conversation, context).is_some()
}

/// Which deterministic check rejected the reply — for the operator's log. Two live
/// turns in a row were diagnosable only by inference because the record showed the
/// rejection's *consequences* (retries, an escalation, the valve) and never its reason.
pub(super) fn why_nothing_useful(
    reply: &ButlerReply,
    conversation: &[TurnMessage],
    context: &ButlerContext,
) -> Option<&'static str> {
    if not_an_answer(reply, context) {
        return Some("not an answer (empty, degraded, plumbing, or described a tool)");
    }
    if repeats_its_last_answer(reply, conversation) {
        return Some("repeats its last answer");
    }
    if gave_up_after_a_failure(reply, conversation) {
        return Some("gave up right after a failure");
    }
    None
}

/// Whether the model stopped trying right after its action failed.
///
/// The third escalation trigger, and like the others it is a check the code applies to the
/// records — never the model's opinion of how it did. The shape it catches was measured
/// before it was written: `verify:failure-names-what-is-really-there` fails 0/3 on the
/// local model — one call fails, the read-back names what actually exists, and the local
/// model answers prose instead of retrying. A deeper model gets one chance at exactly that
/// moment, which is the whole capability ladder as a habit rather than a button.
///
/// Bounded by what already bounds the loop: the failure cap and the round cap. At most a
/// couple of deep attempts per turn, each disclosed as escalations already are, each with
/// nothing personal leaving under its own name.
pub(super) fn gave_up_after_a_failure(reply: &ButlerReply, conversation: &[TurnMessage]) -> bool {
    if !reply.tool_calls.is_empty() {
        return false; // still trying — that is the loop working, not a dead end.
    }
    conversation
        .iter()
        .rev()
        .find_map(|m| match m {
            TurnMessage::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .is_some_and(|last| {
            last.starts_with("error:") || last.contains("reached this turn's action limit")
        })
}

/// Whether this reply is the butler's previous one again.
///
/// Compared on normalized words so trivial re-wording still counts, and only against the
/// **most recent** butler turn — a butler that answers "it's off" twice about the same light
/// on consecutive turns is correct, and only becomes suspect when nothing else was said in
/// between.
pub(super) fn repeats_its_last_answer(reply: &ButlerReply, conversation: &[TurnMessage]) -> bool {
    let text = reply.text.trim();
    // Short confirmations are legitimately identical — "Done." twice is not a failure.
    const LONG_ENOUGH_TO_BE_A_CLAIM: usize = 40;
    if text.len() < LONG_ENOUGH_TO_BE_A_CLAIM || !reply.tool_calls.is_empty() {
        return false;
    }
    conversation
        .iter()
        .rev()
        .find_map(|m| match m {
            TurnMessage::Assistant { text, .. } if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        })
        .is_some_and(|last| words_of(&last) == words_of(text))
}

/// Tool rounds allowed in a **chat** turn. `run_tool_turn` already answers after one
/// tool round (retrying only on failure), so this only bounds a pathological loop —
/// kept low to stay fast on a slow local model.
pub(super) const CHAT_TOOL_ROUNDS: usize = 3;
/// Tool rounds allowed in a proactive **check-in** — it mostly just needs to look.
pub(super) const CHECKIN_TOOL_ROUNDS: usize = 3;
/// Tool rounds allowed in the **nightly loop** — it researches one focus, unhurried.
pub(super) const NIGHTLY_TOOL_ROUNDS: usize = 4;

/// Where a turn's actions are recorded, and what motivated them (ADR 0053).
///
/// Grouped rather than threaded as two more loose parameters, because they are one
/// concern: an outcome and the belief it traces back to. `motivated_by` is `Some` only
/// where the turn genuinely has a reason on file — the nightly loop acting on its focus
/// belief — and `None` when the person simply asked for something.
pub(crate) struct OutcomeSink<'a> {
    pub(super) outcomes: &'a dyn OutcomeRepository,
    motivated_by: Option<BeliefId>,
}

impl<'a> OutcomeSink<'a> {
    /// A sink for a turn with no belief behind it — the person asked.
    pub(crate) const fn unmotivated(outcomes: &'a dyn OutcomeRepository) -> Self {
        Self {
            outcomes,
            motivated_by: None,
        }
    }

    /// A sink for a turn Endora took because of something it believes.
    pub(crate) const fn motivated_by(
        outcomes: &'a dyn OutcomeRepository,
        belief: Option<BeliefId>,
    ) -> Self {
        Self {
            outcomes,
            motivated_by: belief,
        }
    }

    /// Records what an action claimed and what was observed afterwards.
    ///
    /// Skips the `Observe` band: a read changes nothing, so there is no outcome to have
    /// — its result is already evidence (ADR 0053). A capability with no visible spec is
    /// treated as an actuator and recorded, matching the deny-by-default rule elsewhere.
    ///
    /// **Best-effort.** A failed write is swallowed: recording what happened must never
    /// break a working action, the same rule ADR 0053 set for verification.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit dependencies, no hidden state"
    )]
    fn record(
        &self,
        spec: Option<&crate::CapabilitySpec>,
        capability: &str,
        input: &str,
        claim: &str,
        observation: Option<&str>,
        changed: Option<bool>,
        ids: &impl IdSource,
        clock: &impl Clock,
    ) -> Option<OutcomeId> {
        if spec.is_some_and(|s| s.reversibility == Reversibility::Observe) {
            return None;
        }
        let Ok(outcome) = Outcome::record(
            OutcomeId::new(ids.new_id()),
            capability,
            input,
            claim,
            observation,
            clock.now(),
            self.motivated_by,
            changed,
        ) else {
            return None;
        };
        let _ = self.outcomes.save(&outcome);
        Some(outcome.id())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub(crate) fn run_tool_turn(
    butler: &dyn Butler,
    capabilities: &dyn CapabilityRunner,
    audit: &dyn AuditLog,
    actions: &OutcomeSink<'_>,
    ids: &impl IdSource,
    clock: &impl Clock,
    history: &[ChatMessage],
    // What each past reply found, by message id — so asking again does not start from
    // nothing. `&|_| Vec::new()` for a caller with no findings to hand.
    findings: &dyn Fn(&str) -> Vec<String>,
    prefs: &[Preference],
    context: &ButlerContext,
    max_rounds: usize,
    on_step: &mut dyn FnMut(ButlerStep),
    on_token: &mut dyn FnMut(&str),
    activity: &mut Vec<String>,
    disclosures: &mut Vec<ActionDisclosure>,
) -> Result<ButlerReply, AppError> {
    // Stop hammering a dead end: after this many failed runs in a turn, stop executing
    // and let the model answer from what it has.
    const MAX_TOOL_FAILURES: usize = 2;
    // Seed the conversation with the chat so far, **and what it found while having it**.
    let mut conversation: Vec<TurnMessage> = seeded_from(history, findings);
    let mut failures = 0usize;
    // Whether the most recent action errored — see the recovery branch below.
    let mut last_action_failed = false;
    // Tool calls already made this turn (capability + input), to stop the model from
    // looping the same call — especially a read-only one that succeeds every time and
    // so never trips the failure cap.
    // What each call answered, so a repeat gets the answer back instead of a scolding.
    // Whether a stranger has spoken in this turn (ADR 0064). Once third-party prose is in
    // the context, every proposal from here is downstream of it — so every actuator
    // confirms, proven or not. Provenance, never content: nothing inspects what was said,
    // because judging text written to fool a model is an arms race this cannot win.
    let mut seen: std::collections::HashMap<(String, String), String> =
        std::collections::HashMap::new();
    // The turn's own copy, because what it is offered can grow within the turn: asking for
    // a way to do something brings the matching actuators into the tool list and leaves
    // them there (ADR 0060). Cloned rather than threaded back out — the widening lasts for
    // this turn and the next one starts from the same small list.
    let mut context = context.clone();
    for round in 0..=max_rounds {
        let reply =
            take_turn_retrying_empty(butler, &conversation, prefs, &context, on_token, activity)?;
        if reply
            .tool_calls
            .iter()
            .any(|call| call.capability == crate::goal_request::BEGIN_GOAL)
            && context
                .tools
                .iter()
                .any(|tool| tool.id == crate::goal_request::BEGIN_GOAL)
        {
            if reply.tool_calls.len() != 1 {
                return Err(AppError::BadRequest {
                    message: "Declare research separately from other actions.".to_owned(),
                });
            }
            return Ok(reply);
        }
        // No tool call → the final answer, grounded in the tool results so far — unless
        // the last thing that happened was a FAILED action and there is budget left.
        //
        // Observed live, twice: one call failed, the read-back came back naming what is
        // really there, and the model answered "Let's try again… Here is the request:"
        // and stopped. It wrote the preamble to a tool call instead of making one, and
        // the loop took that prose as its final word.
        //
        // This is loop policy, not persuasion: nothing is added to the prompt and the
        // model is told nothing. It simply is not taken at its word that it is finished
        // while the only thing it has done is fail. The existing round and failure caps
        // still bound everything, so a model that really is done just answers again.
        if reply.tool_calls.is_empty() {
            if last_action_failed && round < max_rounds && failures < MAX_TOOL_FAILURES {
                last_action_failed = false;
                conversation.push(TurnMessage::Assistant {
                    text: reply.text.clone(),
                    tool_calls: Vec::new(),
                });
                continue;
            }
            return Ok(reply);
        }
        // Out of rounds with tools still pending — fall through to a forced answer.
        if round == max_rounds {
            break;
        }
        // Record the assistant's tool-call turn, then run each call and append its
        // real result (OpenAI requires the assistant turn before its tool results).
        conversation.push(TurnMessage::Assistant {
            text: reply.text.clone(),
            tool_calls: reply.tool_calls.clone(),
        });
        for call in &reply.tool_calls {
            let id = call.capability.clone();
            // Action budget spent: don't execute more, but tell the model so it wraps
            // up (a system fact fed back as a tool result — not a user-facing string).
            if failures >= MAX_TOOL_FAILURES {
                conversation.push(TurnMessage::ToolResult {
                    call_id: call.id.clone(),
                    content: "not run — reached this turn's action limit; answer with what you \
                              have."
                        .to_owned(),
                });
                continue;
            }
            // Don't loop the same call: if this exact tool + input already ran this turn,
            // hand back **what it said the first time** rather than re-running it.
            // Compared by what the arguments *mean*, not how they are spelled. The
            // capabilities context owns tool-argument knowledge; this layer only asks.
            //
            // It used to hand back an instruction — *"you already called this with the same
            // input this turn — use the earlier result or answer now"* — and a weak model
            // read that as the thing to say. Asked about events at a stadium it called a
            // skill that takes no arguments twice, got the scolding, and answered:
            //
            //     None of the previous calls required inputs, so there's no earlier result
            //     to reuse. Could you please provide more details on what you'd like to
            //     achieve?
            //
            // That is Endora's own bookkeeping, reasoned about out loud and put to the
            // person as a question. A tool result is **data, not instructions**: anything
            // written to the model in the second person is something it may repeat, and
            // [0053](../../docs/adr/0053-honesty-about-what-it-did.md) settled that the
            // guarantee belongs in code rather than in wording the model is trusted to obey.
            //
            // Returning the earlier answer is also simply more use. The nudge told it to
            // "use the earlier result" while withholding it, which asks a 7B model to hold a
            // conversation in its head; this hands it back. Repetition stays bounded by the
            // round and action caps, which is where that job belonged all along.
            let already = (
                id.clone(),
                endora_capabilities::same_call_as(&call.input_json),
            );
            if let Some(said_before) = seen.get(&already) {
                conversation.push(TurnMessage::ToolResult {
                    call_id: call.id.clone(),
                    content: said_before.clone(),
                });
                continue;
            }
            // Asking for a way to do something is not doing it (ADR 0060). It runs no
            // capability, touches nothing, and needs no clearance — it hands back the
            // actuators that were deferred and adds them to what this turn may call.
            //
            // Everything it returns was already deferred, which means it was already
            // *available*: this widens attention, never permission. A tool that was
            // blocked is still blocked when it arrives, and one that acts still asks
            // (ADR 0051).
            if id == LOOK_FOR_A_TOOL {
                let found: Vec<CapabilityTool> = context.deferred.clone();
                let content = if found.is_empty() {
                    "there is nothing here that can do that.".to_owned()
                } else {
                    // Best-first, since that is the order they were put in.
                    let list = found
                        .iter()
                        .map(|t| format!("{} — {}", t.id, t.description))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("these can act, best first — call one of them now:\n{list}")
                };
                for tool in found {
                    if !context.tools.iter().any(|t| t.id == tool.id) {
                        context.tools.push(tool);
                    }
                }
                context.deferred.clear();
                conversation.push(TurnMessage::ToolResult {
                    call_id: call.id.clone(),
                    content,
                });
                continue;
            }
            let spec = capabilities.available().into_iter().find(|c| c.id == id);
            // Not a flag: the proof that no stranger has spoken is derived from the
            // conversation as it stands, each time it is needed (ADR 0070). The bool this
            // replaces was set in one place and read in two, and the class of bug it
            // invited — a third site that forgets it exists — had already shipped once
            // (ADR 0067). Held as `Option`, the absence of the proof IS the taint.
            let no_stranger = crate::egress::NoStrangerSpoke::given(&conversation);
            let cleared = spec
                .as_ref()
                .is_some_and(|s| s.configured && s.autonomous)
                // A read still runs; only the doing is unsafe. The question the person
                // asked still deserves an answer. Acting unasked requires the proof.
                && spec.as_ref().is_some_and(|s| {
                    s.reversibility == Reversibility::Observe || no_stranger.is_some()
                });
            let (status, content) = if cleared {
                on_step(ButlerStep {
                    skill: id.clone(),
                    status: StepStatus::Running,
                    label: progress_label(&id),
                    output: None,
                });
                // Look BEFORE acting, so "did anything change?" is answerable at all.
                // A tool that claims success having changed nothing is the failure
                // ADR 0053 was built for, and comparing two readings settles it without
                // interpreting either — no knowledge of any server's format required.
                // The person's own words, so "did anybody name this place?" has its
                // arbiter (ADR 0065, amended): a follow-up keeps the city its earlier
                // message named, and a city from nowhere is the model recalling.
                let the_person_said: String = conversation
                    .iter()
                    .filter_map(|m| match m {
                        TurnMessage::User(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let input_json = endora_capabilities::place_filled_in(
                    &call.input_json,
                    spec.as_ref().is_some_and(|s| s.wants_place),
                    &context.where_they_are,
                    &the_person_said,
                );
                let before = read_state_back(capabilities, &id, &input_json);
                match capabilities.run(&id, &input_json) {
                    Ok(out) => {
                        last_action_failed = false;
                        activity.push(format!("Used the {id} skill"));
                        // Evidence verifies (ADR 0053): look at the world rather than
                        // taking the actuator's word for what it did.
                        let observed = read_state_back(capabilities, &id, &input_json);
                        // …but look again if it seems nothing moved. A service that
                        // updates state asynchronously answers the call first and changes
                        // the world a moment later, so the first reading races the change
                        // and loses. Observed: a light the person watched go off was
                        // recorded `changed: false`, and the butler then told them it
                        // could not turn it off — Endora asserting a falsehood and the
                        // model faithfully repeating it.
                        //
                        // Only on the "looks unchanged" path, so a change seen straight
                        // away costs nothing, and only once.
                        let observed =
                            if did_change(before.as_deref(), observed.as_deref()) == Some(false) {
                                std::thread::sleep(SETTLE_BEFORE_LOOKING_AGAIN);
                                read_state_back(capabilities, &id, &input_json).or(observed)
                            } else {
                                observed
                            };
                        if observed.is_some() {
                            activity.push(format!("Checked the result of {id}"));
                        }
                        // Memory learns (ADR 0053): the claim and the observation are
                        // kept, apart and unreconciled, so "did that help?" has
                        // something to be answered from later.
                        let recorded = actions.record(
                            spec.as_ref(),
                            &id,
                            &call.input_json,
                            &out,
                            observed.as_deref(),
                            did_change(before.as_deref(), observed.as_deref()),
                            ids,
                            clock,
                        );
                        disclose(
                            disclosures,
                            spec.as_ref(),
                            &id,
                            &out,
                            observed.as_deref(),
                            recorded,
                        );
                        (
                            StepStatus::Done,
                            note_verification_against(
                                &out,
                                spec.as_ref(),
                                before.as_deref(),
                                observed.as_deref(),
                            ) + &note_unchanged(before.as_deref(), observed.as_deref())
                                + &note_changed(before.as_deref(), observed.as_deref())
                                + &note_not_answering(
                                    &call.input_json,
                                    capabilities.current_states(),
                                ),
                        )
                    }
                    Err(e) => {
                        failures += 1;
                        last_action_failed = true;
                        // One greppable line per outcome that is not a plain success.
                        // When a turn went wrong live, the only forensic trail was the
                        // per-message actions blob in the database; the node's log
                        // showed nothing but its own startup.
                        eprintln!("turn: tool {id} failed — {e}");
                        activity.push(format!("Tried the {id} skill, but it failed"));
                        // Read back on failure too: a failed action's most useful
                        // output is what actually exists, which is what lets the
                        // model retry against reality instead of guessing again.
                        //
                        // Deliberately UNSCOPED here, unlike the success path. When an
                        // action fails, its target is the prime suspect — "Area 'Kitchen
                        // Main' does not exist" — so reading back with the same target
                        // fails in exactly the same way and tells the model nothing.
                        // Observed live: the scoped read returned the identical error,
                        // twice, and the butler was left insisting there were no lights
                        // in a kitchen that had five. Widen when something went wrong.
                        let observed = read_state_back(capabilities, &id, "{}");
                        // A failed action is still something that happened, and its
                        // read-back is the most useful thing about it (ADR 0053), so it
                        // is recorded like any other.
                        let recorded = actions.record(
                            spec.as_ref(),
                            &id,
                            &call.input_json,
                            &format!("error: {e}"),
                            observed.as_deref(),
                            // The failure read-back is deliberately unscoped, so it is
                            // not comparable with the scoped `before` — and an action
                            // that errored made no claim to check anyway.
                            None,
                            ids,
                            clock,
                        );
                        disclose(
                            disclosures,
                            spec.as_ref(),
                            &id,
                            &format!("error: {e}"),
                            observed.as_deref(),
                            recorded,
                        );
                        let observed = observed.map_or_else(String::new, |o| {
                            format!(
                                "\n\n[observed] Endora read the state back anyway. \
                                 This is what is actually there:\n{o}"
                            )
                        });
                        (StepStatus::Failed, format!("error: {e}{observed}"))
                    }
                }
            } else {
                // Not cleared to run: audit the policy verdict and hand the model a
                // factual tool result so it answers honestly, in its own voice.
                let decision = capabilities.decision(&id);
                if spec.as_ref().is_some_and(|s| s.configured) {
                    if let Some(summary) = match decision {
                        Some(Decision::Block) => Some(format!(
                            "Policy blocked the '{id}' skill — irreversible and not opened"
                        )),
                        Some(Decision::Confirm) => Some(format!(
                            "Policy required confirmation for the '{id}' skill (not run on its own)"
                        )),
                        _ => None,
                    } {
                        if let Ok(rec) =
                            AuditRecord::new(AuditId::new(ids.new_id()), clock.now(), &summary)
                        {
                            let _ = audit.append(&rec);
                        }
                    }
                }
                let content = match (spec.as_ref(), decision) {
                    (Some(s), _) if !s.configured => {
                        format!("'{id}' is not set up — you can't use it; tell them plainly.")
                    }
                    (Some(_), Some(Decision::Block)) => {
                        format!("'{id}' isn't allowed yet — you can't run it even if asked.")
                    }
                    (Some(s), _)
                        if no_stranger.is_none() && s.reversibility != Reversibility::Observe =>
                    {
                        format!(
                            "'{id}' needs their go-ahead: this turn has read something \
                             written by someone else, so nothing acts on its own until they \
                             say. Ask; don't claim you did it."
                        )
                    }
                    (Some(_), _) => {
                        format!("'{id}' needs their go-ahead — ask; don't claim you did it.")
                    }
                    (None, _) => format!("no such skill '{id}' — you can't do that."),
                };
                eprintln!("turn: tool {id} refused — {content}");
                activity.push(format!(
                    "Couldn't use {id} (off, not set up, or needs confirming)"
                ));
                (StepStatus::Blocked, content)
            };
            on_step(ButlerStep {
                skill: id.clone(),
                status,
                label: progress_label(&id),
                output: Some(content.clone()),
            });
            // From here on, this turn may not act on its own — and the result carries the
            // mark, so the escalation decision downstream can see it too.
            // A call that FAILED returned no stranger's words, so there is nothing to be
            // tainted by — and tainting anyway blocked the escalation that exists to catch
            // exactly this. Seen live within an hour of shipping both: the mail skill
            // failed, the turn was marked as having heard from outside, escalation was
            // refused, and the local model told the person there was no new mail. The rule
            // silenced the mechanism meant to notice it was lying.
            let content =
                if spec.as_ref().is_some_and(|s| s.third_party) && status != StepStatus::Failed {
                    format!("{STRANGER_MARK}{content}")
                } else {
                    content
                };
            // Remember what it answered, so calling it again this turn gets this back
            // rather than a scolding the model might repeat to the person.
            seen.insert(already, content.clone());
            conversation.push(TurnMessage::ToolResult {
                call_id: call.id.clone(),
                content,
            });
        }
        // Let the model decide when it's done: the next round it either calls another
        // tool (e.g. it read state first and now acts on it) or answers with no tool
        // call, which exits the loop above. We do NOT force an answer after one round —
        // that killed the read-then-act pattern (a "turn off the light" that first
        // checked state would stop at the check). The loop stays bounded regardless by
        // MAX_TOOL_ROUNDS, the failure cap, and the repeated-call guard, so a slow model
        // still can't run away.
    }
    // Reached here after acting (or the round cap): force the final answer with tools
    // OFF, so the model replies in prose grounded in what it gathered rather than
    // calling yet another tool and leaving the turn without an answer.
    let mut final_ctx = context.clone();
    final_ctx.tools = Vec::new();
    take_turn_retrying_empty(butler, &conversation, prefs, &final_ctx, on_token, activity)
}

/// Sends a message to the butler and records both turns.
///
/// Appends the person's message, runs the butler's turn, records the reply, and
/// returns it together with a plain-language trail of what the butler did.
///
/// # Errors
/// [`AppError::Domain`] if the message text is blank, [`AppError::Model`] if the
/// butler brain is unavailable, or [`AppError::Repository`] if persistence fails.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn send_to_butler(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    specimens: &impl SpecimenRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
) -> Result<(ChatMessage, Vec<String>), AppError> {
    // Non-streaming is just streaming with the tokens and steps discarded. Deep
    // escalation is wired on the streaming (primary chat) path; None here.
    send_to_butler_streaming(
        chat,
        preferences,
        beliefs,
        outcomes,
        specimens,
        capabilities,
        butler,
        audit,
        None,
        ids,
        clock,
        context,
        text,
        &mut |_| {},
        &mut |_| {},
        &mut Vec::new(),
    )
}

/// Non-streaming turn variant that checkpoints deterministic goal execution.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn send_to_butler_with_goals(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    specimens: &impl SpecimenRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    goals: &dyn GoalRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
) -> Result<(ChatMessage, Vec<String>), AppError> {
    send_to_butler_streaming_with_goals(
        chat,
        preferences,
        beliefs,
        outcomes,
        specimens,
        capabilities,
        butler,
        audit,
        None,
        Some(goals),
        ids,
        clock,
        context,
        text,
        &mut |_| {},
        &mut |_| {},
        &mut Vec::new(),
    )
}

/// Where a [`ButlerStep`] is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// The skill is running right now.
    Running,
    /// The skill returned a result.
    Done,
    /// The skill ran but failed to produce a result.
    Failed,
    /// The skill couldn't run — off, not set up, or it needs confirmation.
    Blocked,
}

impl StepStatus {
    /// A stable lowercase tag for the wire/UI.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}

/// One live step in the butler's agentic loop, surfaced to the UI as it works —
/// a Claude-Code-style expandable action trail, kept distinct from the reply
/// prose (which streams separately as tokens). Steps are emitted sequentially:
/// a skill's [`StepStatus::Running`] is always followed by its terminal
/// [`Done`](StepStatus::Done)/[`Failed`](StepStatus::Failed); a
/// [`Blocked`](StepStatus::Blocked) step is terminal on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButlerStep {
    /// The capability id this step concerns (e.g. `weather`).
    pub skill: String,
    /// Where the step is in its lifecycle.
    pub status: StepStatus,
    /// A short, present-tense label ("Checking the weather").
    pub label: String,
    /// The skill's raw result on a terminal step, for the UI to reveal on demand
    /// (what the butler actually got back). `None` while running or when blocked.
    pub output: Option<String>,
}

/// Records an actuation for the person to see, whatever the reply ends up saying
/// (ADR 0053).
///
/// Skips the `Observe` band on the same reasoning as the outcome record: a read changes
/// nothing, so there is nothing to disclose about it. A capability with no visible spec
/// is treated as an actuator, matching deny-by-default everywhere else.
pub(super) fn disclose(
    disclosures: &mut Vec<ActionDisclosure>,
    spec: Option<&crate::CapabilitySpec>,
    skill: &str,
    claimed: &str,
    observed: Option<&str>,
    outcome: Option<OutcomeId>,
) {
    if spec.is_some_and(|s| s.reversibility == Reversibility::Observe) {
        return;
    }
    // Deliberately NOT trimmed down when the reading is unchanged, unlike what the model
    // is sent. The two have opposite needs: the model gets a reading it must not answer
    // from, and a person gets the evidence. "The switch is still on" next to a claim of
    // `action_done` is the whole disclosure — replacing it with "nothing changed" would
    // take away the fact and leave the verdict, which is the wrong way round (ADR 0053).
    disclosures.push(ActionDisclosure {
        skill: skill.to_owned(),
        claimed: claimed.trim().to_owned(),
        observed: observed.map(|o| o.trim().to_owned()),
        outcome: outcome.map(|id| id.value().to_string()),
    });
}

/// What to say when the model produced no words but the turn did something.
///
/// Observed: a light was switched on by direct reach, the model returned an empty reply,
/// and Endora answered "I'm not sure how to help with that yet" — an apology for work it
/// had just completed. The person then cannot tell whether to try again.
///
/// The tool's own report is quoted and attributed rather than paraphrased, because Endora
/// is entitled to say what a tool told it and not to invent a summary of it.
pub(super) fn acted_note(disclosures: &[ActionDisclosure]) -> Option<String> {
    let done: Vec<&ActionDisclosure> = disclosures
        .iter()
        .filter(|d| !d.claimed.trim_start().starts_with("error:"))
        .collect();
    let first = done.first()?;
    // The claim's opening line: enough to say what happened, without pasting a reading.
    let said = first
        .claimed
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("it reported success")
        .trim();
    let more = match done.len() {
        1 => String::new(),
        n => format!(" ({} actions in all — the trail below has each one.)", n),
    };
    Some(format!("Done. {} reported: {said}{more}", first.skill))
}

/// The facts behind an answer, for the things it named.
///
/// ADR 0053 verifies what Endora **did** and has never verified what it **says**. On a turn
/// that answers a question, Endora is holding the very reading the answer came from — so
/// the facts were available all along and simply never shown. Live, that gap produced
/// "the kitchen table light is already on" about a light that was off, and "several lights
/// are on" when the reading listed exactly nine.
///
/// This does not judge the prose. It **discloses** — the same move the action trail makes,
/// applied to an answer: whatever the reply named, here is what the service says about it,
/// so the person can see in one glance whether the two agree. Correcting the model would
/// mean understanding the sentence; showing the facts does not.
///
/// Only names the reply actually mentions, longest first so `Kitchen Main Light` is
/// matched before `Kitchen`, and capped — a wall of state is its own kind of noise.
pub(super) fn facts_behind(asked: &str, reply: &str, mut states: Vec<(String, String)>) -> String {
    const SHOWN: usize = 5;
    const WORTH_MATCHING: usize = 4;
    let in_request = words_of(asked);
    let in_reply = words_of(reply);
    // A reply that asserts nothing about state needs no facts about state. Live, under a
    // greeting — "How can I assist you with your home automation setup?" — this appended
    // `[state] Home is 0`, because the house contains something called `Home` and the word
    // "home" was in the sentence. Technically an exact match, and completely meaningless.
    //
    // The vocabulary for "is this about state?" comes from the **reading**, never from a
    // list of English words: whatever values this service just reported are exactly the
    // words a reply asserting state would use. A service that reports `open`/`closed` is
    // handled by the same code as one reporting `on`/`off`, and one whose vocabulary
    // Endora has never seen contributes nothing — the same rule as ADR 0054's categories.
    let reply_asserts_a_state = states
        .iter()
        .any(|(_, state)| contains_all_words(&in_reply, &words_of(state)));
    states.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut said: Vec<String> = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    for (name, state) in states {
        if name.len() < WORTH_MATCHING || said.len() >= SHOWN {
            continue;
        }
        // Whole words, not a substring: `Home` must not match "homework", and `Den` must
        // not match "identify".
        let name_words = words_of(&name);
        // Two ways in, and they are gated differently on purpose.
        //
        // **The person named it** — no gate. They asked about this thing, so its state is
        // the answer they are waiting for, whatever the reply managed to say. Live: asked
        // "is the kitchen main light on?", the butler answered that it "does not appear to
        // be in your home setup" while the light was on, and nothing contradicted it
        // because the reply contained no state word to trip the gate.
        //
        // **The model mentioned it** — gated, because prose wanders. That gate is what
        // stops `[state] Home is 0` appearing under a greeting.
        let matched = contains_all_words(&in_request, &name_words)
            || (reply_asserts_a_state && contains_all_words(&in_reply, &name_words));
        if !matched {
            continue;
        }
        // Skip a name already contained in a longer one that was matched, so a turn about
        // `Kitchen Main Light` does not also report `Kitchen`.
        if name_words.iter().all(|w| covered.contains(w)) {
            continue;
        }
        covered.extend(name_words);
        said.push(format!("{name} is {state}"));
    }
    if said.is_empty() {
        return String::new();
    }
    format!("\n\n[state] {}", said.join(" · "))
}

/// Splits text into lowercase alphanumeric words, so matching happens on whole words.
pub(super) fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `words` contains `wanted` as a **contiguous run** of whole words.
pub(super) fn contains_all_words(words: &[String], wanted: &[String]) -> bool {
    if wanted.is_empty() || wanted.len() > words.len() {
        return false;
    }
    words.windows(wanted.len()).any(|run| run == wanted)
}

/// Appends what Endora actually did, when the person asked about it.
///
/// The third attempt at this, and the first that does not depend on the model. Asked "did you
/// do anything while I was out?", the butler:
///
/// 1. answered out of the house's lights — so Endora's record became a **skill**;
/// 2. reached past that skill and called `HassTurnOn`, trying to switch a light on — so the
///    account was **stated in the turn's context** instead;
/// 3. ignored the context and repeated, word for word, the answer it had given the previous
///    day: *"no specific activities were recorded."*
///
/// Eleven things were in the record at that moment, including the three messages it had sent
/// unprompted. Each fix was a better place to put the facts, and all three were still asking
/// the model to use them.
///
/// So this is the mechanism [0053](../../docs/adr/0053-honesty-about-what-it-did.md) already
/// uses for exactly this shape of problem: **a claim and the record disagree, so the record is
/// appended.** Nothing is rewritten and nothing is judged — the account is put next to the
/// sentence and the person can see which to believe.
///
/// **The trigger is what the person asked, never how the model answered.** It was first keyed
/// on a claim in the reply — `activity`, `activities`, `proactive`, `recorded` — and measured
/// live a pleasantry ("…other events or activities, feel free to ask!") then appended the whole
/// record, including a message sent that morning, to an answer about a city. A model's phrasing
/// is the one thing this cannot rest on: anything keyed on it fails about a third of the time.
/// The question is stable, it is their own words, and it is the thing they are waiting to hear
/// about — and it needs no particular reply, so a flat "no" is answered with the record too.
pub(super) fn account_behind(asked: &str, did_lately: &[String]) -> String {
    if did_lately.is_empty() || !asks_what_it_did(asked) {
        return String::new();
    }
    format!("\n\n[did] {}", did_lately.join("\n"))
}

/// Whether the person is asking about Endora's OWN doings: "did you do anything while I was
/// out?", "what have you been up to?", "any proactive actions today?".
///
/// Deliberately about *you* and about *doing*, not about a topic. The word "activities" is not
/// a cue — "find me some activities for the kids" is a request, and it was the very word that
/// caused the leak. A heuristic, named as one, and both ways of being wrong are cheap: a false
/// positive appends true facts to an answer that did not need them, and a false negative leaves
/// things as they were — but it can no longer be set off by what the model happens to say.
pub(super) fn asks_what_it_did(text: &str) -> bool {
    let words = words_of(text);
    let has = |wanted: &str| words.iter().any(|w| w == wanted);
    // Words about Endora's own initiative, whoever says them.
    if has("proactive") || has("unprompted") {
        return true;
    }
    const DOING: &[&str] = &["did", "done", "doing", "been", "lately"];
    has("you") && DOING.iter().any(|w| has(w))
}

/// The line to append when a turn tried to change something and changed nothing.
///
/// Empty in every other case — including a turn that took no action at all, where there
/// is nothing to correct, and a turn where anything succeeded, where "nothing changed"
/// would itself be false.
///
/// Only actuating actions are considered, because reads never reach the disclosure list.
pub(super) fn nothing_changed_note(disclosures: &[ActionDisclosure]) -> String {
    let tried_and_failed = !disclosures.is_empty()
        && disclosures
            .iter()
            .all(|d| d.claimed.trim_start().starts_with("error:"));
    if !tried_and_failed {
        return String::new();
    }
    // The person gets what the model was given. When a name did not match, Endora already
    // worked out what does exist; sending that only to the model wasted it — observed, the
    // reply offered to "check the living room instead" while the shortlist sat unread in
    // the tool result.
    let names = candidates_offered(disclosures);
    if names.is_empty() {
        return "\n\n(Nothing was changed — everything I tried failed.)".to_owned();
    }
    format!(
        "\n\n(Nothing was changed — everything I tried failed. These exist and look like \
         what you asked for: {}.)",
        names.join(", ")
    )
}

/// The names the target search offered this turn, lifted back out of the tool results.
///
/// Reading Endora's own marker, not a server's format — the one place that is fair game,
/// because this text was written here.
pub(super) fn candidates_offered(disclosures: &[ActionDisclosure]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for claimed in disclosures.iter().map(|d| d.claimed.as_str()) {
        let Some(block) = claimed.split("[candidates]").nth(1) else {
            continue;
        };
        for line in block.lines() {
            let Some(name) = line.trim().strip_prefix("- ") else {
                continue;
            };
            let name = name.trim();
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
    }
    names.truncate(4);
    names
}

/// One action a turn took, and whether Endora saw the effect for itself (ADR 0053).
///
/// The **deterministic** half of honesty about actions. [ADR 0053](../../docs/adr/0034-evidence-verifies.md)
/// put the read-back in the tool result and asked the model to respect it; measured, a
/// weak local model ignores that instruction — it asserts an unverified success every
/// time. So the guarantee stops being *"the butler will report this honestly"* (a claim
/// about a model) and becomes *"the person can always see it"* (a claim about code).
///
/// This never touches `reply.text`. The butler writes what it writes; this sits beside
/// it, the same way the activity trail already does. Putting words in the butler's mouth
/// is what ADR 0053 forbids — showing the person what happened is not that.
///
/// It derives no verdict. Deciding *contradicted* versus *confirmed* needs a model of
/// what the caller intended, which does not exist (ADR 0053); the claim and the reading
/// are shown side by side and the person judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDisclosure {
    /// The capability that acted.
    pub skill: String,
    /// The actuator's own account of its work — success text, or the error.
    pub claimed: String,
    /// What Endora observed afterwards, when the integration has a reader. `None` means
    /// nothing could check — which is the fact worth surfacing, not a gap to hide.
    pub observed: Option<String>,
    /// The stored outcome this disclosure describes, so the person can say how it landed
    /// from **where they already are** (ADR 0053).
    ///
    /// `None` when nothing was recorded — a read, or a failed write. The machinery for
    /// judging an outcome was complete for months and had never once been used, because
    /// the only place to do it was a section further down a screen nobody opens, which by
    /// its own design never asked. A loop with no input is not a loop.
    pub outcome: Option<String>,
}

impl ActionDisclosure {
    /// Whether Endora saw the effect for itself rather than only being told about it.
    #[must_use]
    pub const fn was_observed(&self) -> bool {
        self.observed.is_some()
    }
}

/// Bounds the prompt for a long chat (ADR 0053): returns the recent verbatim window
/// plus a compact running summary of everything before it. The summary is extended
/// (one model call) only when new messages have scrolled past the window since it was
/// last built; otherwise the cached summary is reused. If summarising isn't available
/// (the default butler) or fails, the recent window still stands — just without the
/// earlier context folded in.
/// The recent messages and running summary the next turn would send, without running the
/// summariser: the same arithmetic as the turn, read-only, for the console's account of what
/// is in the window (ADR 0088). `from_ms` is where the current topic begins.
///
/// # Errors
/// [`AppError::Repository`] if the chat cannot be read.
pub fn history_the_next_turn_would_send(
    chat: &impl ChatRepository,
    summary: Option<ConversationSummary>,
    from_ms: i64,
) -> Result<(Vec<ChatMessage>, Option<String>), AppError> {
    const RECENT_WINDOW: usize = 12;
    const SUMMARY_BATCH: usize = 12;
    let full: Vec<ChatMessage> = chat
        .list()?
        .into_iter()
        .filter(|m| m.at().unix_millis() >= from_ms)
        .collect();
    if full.len() <= RECENT_WINDOW {
        return Ok((full, None));
    }
    let current = summary.unwrap_or_default();
    let start = current
        .covered
        .max(full.len().saturating_sub(RECENT_WINDOW + SUMMARY_BATCH));
    let text = (!current.text.is_empty()).then_some(current.text);
    Ok((full[start..].to_vec(), text))
}

pub(super) fn compact_history(
    butler: &dyn Butler,
    summary: &dyn ConversationSummaryStore,
    full: &[ChatMessage],
    window: usize,
) -> (Vec<ChatMessage>, Option<String>) {
    if full.len() <= window {
        return (full.to_vec(), None);
    }
    // Fold the overflow into the summary only once enough new messages have piled up —
    // NOT every turn. Each turn adds messages, so the window boundary advances every
    // turn; summarising on every advance would pay a slow model call per turn on a long
    // chat (which is exactly what degraded late turns).
    const SUMMARY_BATCH: usize = 12;
    // Hard cap on how many recent messages ride verbatim, so the prompt can't blow up
    // even when the summary is behind: the tail window plus at most one pending batch.
    let max_verbatim = window + SUMMARY_BATCH;
    let boundary = full.len() - window; // messages [0..boundary) are older than the tail window
    let mut current = summary.get().unwrap_or_default();
    if boundary >= current.covered + SUMMARY_BATCH {
        // Fold exactly ONE batch per turn, not the whole backlog. A big backlog — e.g.
        // after a restart cleared the in-memory summary — is then caught up over a few
        // turns, each a small, fast call rather than one giant summarisation that would
        // itself time out on a slow model.
        let end = current.covered + SUMMARY_BATCH;
        let chunk = render_transcript(&full[current.covered..end]);
        if let Ok(s) = butler.summarize(&current.text, &chunk) {
            let s = s.trim();
            if !s.is_empty() {
                current = ConversationSummary {
                    text: s.to_owned(),
                    covered: end,
                };
                summary.set(current.clone());
            }
        }
        // Summariser unavailable/failed: keep the prior summary and coverage; the cap
        // below still bounds the prompt.
    }
    let text = (!current.text.is_empty()).then_some(current.text);
    // Verbatim = the not-yet-summarised tail, but never more than `max_verbatim`. If the
    // summary is lagging (summariser failing), drop the oldest beyond the cap rather than
    // send a giant prompt — lost detail beats a timeout, and durable facts live in
    // beliefs, not the transcript.
    let start = current.covered.max(full.len().saturating_sub(max_verbatim));
    (full[start..].to_vec(), text)
}

/// Renders messages as a plain `User:/Butler:` transcript, for summarising.
pub(super) fn render_transcript(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            let who = match m.role() {
                MessageRole::User => "User",
                MessageRole::Butler => "Butler",
            };
            format!("{who}: {}", m.text())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Like [`send_to_butler`], but streams the reply's prose to `on_token` as the
/// butler produces it (for a live, token-by-token chat). The person's message is
/// persisted **before** the butler is called, and the butler's reply is persisted
/// once complete — so the exchange survives a reload even if the stream is
/// interrupted mid-way (the last stored message is then still the person's).
///
/// # Errors
/// [`AppError::Model`] if the butler fails, or [`AppError::Repository`] on a
/// backend failure.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn send_to_butler_streaming(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    specimens: &impl SpecimenRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    summary: Option<&dyn ConversationSummaryStore>,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
    on_token: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(ButlerStep),
    disclosures: &mut Vec<ActionDisclosure>,
) -> Result<(ChatMessage, Vec<String>), AppError> {
    send_to_butler_streaming_with_goals(
        chat,
        preferences,
        beliefs,
        outcomes,
        specimens,
        capabilities,
        butler,
        audit,
        summary,
        None,
        ids,
        clock,
        context,
        text,
        on_token,
        on_step,
        disclosures,
    )
}

/// Streaming turn variant that can declare durable read-only work.
/// The ordinary public function remains unchanged for clients that do not wire
/// a goal repository.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn send_to_butler_streaming_with_goals(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    specimens: &impl SpecimenRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    summary: Option<&dyn ConversationSummaryStore>,
    goals: Option<&dyn GoalRepository>,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
    on_token: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(ButlerStep),
    disclosures: &mut Vec<ActionDisclosure>,
) -> Result<(ChatMessage, Vec<String>), AppError> {
    send_to_butler_streaming_for_request(
        chat,
        preferences,
        beliefs,
        outcomes,
        specimens,
        capabilities,
        butler,
        audit,
        summary,
        goals,
        ids,
        clock,
        context,
        text,
        None,
        on_token,
        on_step,
        disclosures,
    )
}

/// Intake with an optional stable client request ID for safe transport retries.
#[expect(clippy::too_many_arguments, reason = "explicit dependencies")]
pub fn send_to_butler_streaming_for_request(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    specimens: &impl SpecimenRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    summary: Option<&dyn ConversationSummaryStore>,
    goals: Option<&dyn GoalRepository>,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
    request_id: Option<&str>,
    on_token: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(ButlerStep),
    disclosures: &mut Vec<ActionDisclosure>,
) -> Result<(ChatMessage, Vec<String>), AppError> {
    let request_key = if let Some(id) = request_id {
        if id.is_empty()
            || id.len() > 120
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(AppError::BadRequest {
                message: "request_id must be 1–120 letters, digits, hyphens or underscores"
                    .to_owned(),
            });
        }
        format!("request-{id}")
    } else {
        goal_key_for(text)
    };
    if let Some(goals) = goals {
        if let Some(existing) = goals.get_goal(&request_key)? {
            if existing.goal().objective() != text.trim() {
                return Err(AppError::Conflict {
                    code: "request_key_reused",
                    message: "This request ID belongs to different work.".to_owned(),
                });
            }
            let answer = if existing.goal().status() == crate::GoalStatus::Complete {
                existing
                    .goal()
                    .evidence()
                    .last()
                    .map(|e| e.output.clone())
                    .unwrap_or_else(|| "This request is complete.".to_owned())
            } else {
                format!(
                    "This request is {}: {}",
                    existing.goal().status().name(),
                    existing.goal().reason()
                )
            };
            on_token(&answer);
            return Ok((
                post_butler_message(chat, ids, clock, &answer)?,
                vec!["Reused the existing goal".to_owned()],
            ));
        }
    }
    let user = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        text,
        clock.now(),
    )?;
    // Read the history BEFORE the new message joins it, so "what was asked last time" is the
    // previous request rather than this one.
    let asked_before = chat.list()?;
    chat.append(&user)?;

    // CONTEXT COMPACTION (ADR 0053): send the model a bounded prompt — a compact running
    // summary of earlier turns + a recent verbatim window — not the whole transcript.
    // A big prompt slows a local model past the timeout, so a long chat would otherwise
    // degrade; but a butler shouldn't forget the day, so the overflow is summarised
    // rather than dropped. The summary is regenerated only when the window overflows
    // FURTHER (new messages scroll past it), so most turns pay nothing.
    const RECENT_WINDOW: usize = 12;
    // Only what is after the person's last "new topic" is the model's to read.
    let full: Vec<ChatMessage> = chat
        .list()?
        .into_iter()
        .filter(|m| m.at().unix_millis() >= context.context_from_ms)
        .collect();
    let (history, summary_text) = match summary {
        Some(store) => compact_history(butler, store, &full, RECENT_WINDOW),
        // No summary store (non-streaming path/tests): just bound with the recent
        // window, no summarising.
        None => (
            full.iter()
                .skip(full.len().saturating_sub(RECENT_WINDOW))
                .cloned()
                .collect(),
            None,
        ),
    };
    let mut context = context.clone();
    context.conversation_summary = summary_text;
    // What Endora did lately is given to the model only when the person asks about it. Kept
    // in front of it all the time it was quoted back into answers about something else
    // (a sports question that ended in "[did] 18:49 — I wrote to you unprompted…").
    if !asks_what_it_did(text) {
        context.did_lately.clear();
    }
    let context = &context;
    let prefs = preferences.list_all()?;

    // `activity` — a plain-language record of what the butler did this turn.
    let mut activity: Vec<String> = Vec::new();
    // If this message is the last one again, the previous turn's actions did not work — and
    // the person has just said so without being asked (ADR 0052).
    if let Ok(n) = note_what_the_repeat_says(outcomes, &asked_before, text, clock.now()) {
        if n > 0 {
            activity.push(format!(
                "You asked again, so I marked {n} thing{} as not having helped",
                if n == 1 { "" } else { "s" }
            ));
        }
    }
    // They asked, so nothing on file motivated it (ADR 0053).
    let actions = OutcomeSink::unmotivated(outcomes);

    // STANDING QUESTIONS ROUTE DETERMINISTICALLY (ADR 0074/0076). A known ask for
    // a known feature is policy's to route, not the model's to guess — measured
    // live twice: the first brief ask after its skill shipped went to house gossip,
    // and "what lights are on" survived four fixes of everything around the model
    // before the summary itself was taken from it. Falls through to the ordinary
    // turn when nothing matches or the answer cannot be assembled.
    if let Some(reply_text) = standing_answer(
        text,
        capabilities,
        butler,
        goals,
        ids,
        clock,
        on_token,
        on_step,
        &mut activity,
    ) {
        let message = post_butler_message(chat, ids, clock, &reply_text)?;
        return Ok((message, activity));
    }

    // SINGLE TOOL-CALLING CONVERSATION (ADR 0053): the butler runs its tools through
    // policy and answers grounded in their real results — success or error — with no
    // deterministic narration. A failed tool comes back as a factual tool result the
    // model must relay honestly; if it misreports with the truth in front of it, that
    // is a model failure to surface, not a canned string to hardcode. The proactive
    // flows (check-in, brief, nightly loop) run on this same loop.
    let reply = {
        let mut relay = |chunk: &str| on_token(chunk);
        // What each past reply found, given back to the butler (ADR 0053's trail, read
        // rather than only displayed). A read that fails is not worth failing a turn for:
        // the worst case is the conversation it had before this change.
        let findings = |id: &str| chat.what_it_found(id).unwrap_or_default();
        let intake = crate::goal_request::IntakeRunner(capabilities);
        let intake_context = crate::goal_request::intake_context(context);
        run_tool_turn(
            butler,
            if goals.is_some() {
                &intake
            } else {
                capabilities
            },
            audit,
            &actions,
            ids,
            clock,
            &history,
            &findings,
            &prefs,
            if goals.is_some() {
                &intake_context
            } else {
                context
            },
            CHAT_TOOL_ROUNDS,
            on_step,
            &mut relay,
            &mut activity,
            disclosures,
        )?
    };
    if let Some(call) = reply
        .tool_calls
        .iter()
        .find(|call| call.capability == crate::goal_request::BEGIN_GOAL)
    {
        let goals = goals.ok_or_else(|| AppError::BadRequest {
            message: "Durable goals are unavailable.".to_owned(),
        })?;
        let value: serde_json::Value =
            serde_json::from_str(&call.input_json).map_err(|_| AppError::BadRequest {
                message: "Invalid goal declaration.".to_owned(),
            })?;
        let criteria = crate::goal_request::GoalCriteria::from_json(&value).map_err(|message| {
            AppError::BadRequest {
                message: message.to_owned(),
            }
        })?;
        if goals.active_goals()?.len() >= MOST_GOALS_AT_ONCE {
            return Err(AppError::Conflict {
                code: "goal_capacity",
                message: "Three requests are already running; stop one or let it finish."
                    .to_owned(),
            });
        }
        let description = criteria.description();
        let goal =
            crate::Goal::for_request(text, criteria).map_err(|message| AppError::BadRequest {
                message: message.to_owned(),
            })?;
        let record =
            crate::GoalRecord::new(&request_key, goal, clock.now()).map_err(|message| {
                AppError::BadRequest {
                    message: message.to_owned(),
                }
            })?;
        let created = goals.create_goal(&record).map_err(|error| match error {
            crate::RepositoryError::Backend(ref reason)
                if reason == crate::goal_request::GOAL_CAPACITY_REACHED =>
            {
                AppError::Conflict {
                    code: "goal_capacity",
                    message: "Three requests are already running; stop one or let it finish."
                        .to_owned(),
                }
            }
            other => other.into(),
        })?;
        if !created {
            let answer =
                "This request is already queued or finished. Its existing goal was preserved.";
            on_token(answer);
            return Ok((
                post_butler_message(chat, ids, clock, answer)?,
                vec!["Reused the existing goal".to_owned()],
            ));
        }
        let answer = format!("I've started working on {description}. I'll post the result here.");
        on_step(ButlerStep {
            skill: "goal".to_owned(),
            status: StepStatus::Done,
            label: "Started durable work".to_owned(),
            output: Some(description),
        });
        on_token(&answer);
        return Ok((
            post_butler_message(chat, ids, clock, &answer)?,
            vec![format!("Started goal {request_key}")],
        ));
    }

    // A local answer that says nothing was found is checked once more, by the deep model
    // choosing the skills (ADR 0087). Only on a turn that did not act, only when the person
    // has turned automatic escalation on (`deeper` is `None` otherwise, ADR 0055), and the
    // retry is the same Ask-deep arrangement as the button (ADR 0086): the deep model plans
    // and never sees a result, the local model reads. If the retry fails or says nothing,
    // the local answer stands.
    let mut retried = false;
    // Looked nothing up: no skill ran this turn, so any fact in the answer is from the
    // model's memory. For a question about the world that is a guess, however sure it sounds.
    let looked_nothing_up = !activity.iter().any(|a| {
        a.starts_with("Used the ") || a.starts_with("Tried the ") || a.starts_with("Couldn't use ")
    });
    let reply = if disclosures.is_empty()
        && !reply.escalated
        && (says_it_found_nothing(&reply.text)
            || (looked_nothing_up && butler.deeper().is_some() && needs_a_lookup(butler, text)))
    {
        match butler.deeper() {
            Some(deeper) => {
                // Shown as a step, so the answer already on screen is filed away as working
                // notes instead of being overwritten.
                on_step(ButlerStep {
                    skill: "deep".to_owned(),
                    status: StepStatus::Done,
                    label: "That wasn't looked up, so the deep model is choosing the skills"
                        .to_owned(),
                    output: None,
                });
                let planned = crate::deep_turn::PlannedByTheDeepModel::new(&deeper, butler);
                let mut again: Vec<String> = Vec::new();
                let findings = |id: &str| chat.what_it_found(id).unwrap_or_default();
                match run_tool_turn(
                    &planned,
                    capabilities,
                    audit,
                    &actions,
                    ids,
                    clock,
                    &history,
                    &findings,
                    &prefs,
                    context,
                    CHAT_TOOL_ROUNDS,
                    on_step,
                    &mut |_| {},
                    &mut again,
                    &mut Vec::new(),
                ) {
                    Ok(second) if !second.text.trim().is_empty() => {
                        activity.push(
                            "The local answer wasn't looked up, so the deep model chose the skills"
                                .to_owned(),
                        );
                        activity.extend(again);
                        retried = true;
                        second
                    }
                    _ => reply,
                }
            }
            None => reply,
        }
    } else {
        reply
    };

    // There is exactly one way to the deep model, and it is inside the turn (ADR 0067).
    //
    // A second rung used to live here: when the local model came up empty, it asked the deep
    // model the person's raw sentence. It carried no system prompt, no place, no time and
    // none of what the turn had just found, so a large model answered as a generic assistant
    // — and, worse, it answered on turns the turn itself had *refused* to escalate. The
    // refusals it overrode were the taint rule (ADR 0064) and the person's own consent to
    // automatic escalation (ADR 0055), both of which are checked in `run_tool_turn` and
    // neither of which existed here.
    //
    // A turn that ended in a failure the code itself judged files a SPECIMEN
    // (ADR 0075) — the ask and the verdict, kept in the house's own record so the
    // nightly loop can re-ask it and notice when it starts passing. Only
    // answer-shaped failures: a turn that acted already has an outcome record, and
    // replaying an action unattended would be a different mechanism entirely. The
    // verdict is the same deterministic check that gated retries and escalation;
    // the model's opinion of itself files nothing. Best-effort — a full shelf or a
    // failed write must never break the reply.
    if disclosures.is_empty() {
        if let Some(why) = specimen_verdict(&reply, context) {
            let filed = specimens
                .file_specimen(
                    &ids.new_id().to_string(),
                    text,
                    why,
                    clock.now().unix_millis(),
                )
                .unwrap_or(false);
            if filed {
                activity.push("Couldn't answer this one — kept it to retry overnight".to_owned());
            }
        }
    }

    // Whether the answering round spoke for itself. When it did, that text has already
    // reached the person token by token; when it did not, whatever stands in for it is
    // new to them.
    let answered_in_its_own_words = !reply.text.trim().is_empty() && !retried;
    let reply_text = if reply.text.trim().is_empty() {
        // If the turn ACTED, apologising is simply false — it did something and knows what.
        // ADR 0053 rejected deterministic narration because code-written sentences got
        // contradicted by the model; there is nothing to contradict when the model produced
        // no sentence at all, which is what makes this safe rather than a relapse.
        acted_note(disclosures).unwrap_or_else(|| {
            "I'm not sure how to help with that yet — can you say a bit more?".to_owned()
        })
    } else {
        without_the_record_unless_asked(reply.text.trim(), text)
    };
    // Say it plainly when a turn changed nothing (ADR 0053). Observed: asked to turn on
    // the kitchen table, the only action failed, and the reply announced that "the guest
    // bedroom left lamp is already on" — a device from earlier in the conversation that
    // this turn never touched. The activity trail showed the failure; the sentence the
    // person actually reads did not.
    //
    // Deterministic, and it does not rewrite what the model said: it appends what is
    // true. Whether the model narrates well is not something Endora can fix, but whether
    // the person is told nothing happened is.
    let mut appended = nothing_changed_note(disclosures);
    // Outside every other branch on purpose. This is about what the person ASKED for, so it
    // does not depend on whether the turn acted, succeeded, or answered — and a dead device
    // is precisely the case where an action *is* attempted and fails, which is the branch it
    // was first written into the wrong side of.
    appended.push_str(&note_not_answering(text, capabilities.current_states()));
    // On a turn that answered rather than acted, show the facts behind whatever it named
    // (ADR 0053). Scoped to answers because that is where a claim about state goes
    // unchecked, and because an acting turn already discloses its own before-and-after.
    if disclosures.is_empty() {
        appended.push_str(&facts_behind(
            text,
            &reply_text,
            capabilities.current_states(),
        ));
        // Matched against what the PERSON asked, never against how the model answered. That
        // is the rule `note_not_answering` above already follows, and this was the one call
        // that broke it: keyed on the reply, a pleasantry containing "activities" appended
        // the whole record to an answer about a city. The request is stable — it is the
        // person's own words, and the thing they are waiting to hear about.
        appended.push_str(&account_behind(text, &context.did_lately));
    }
    // Where the words went. Disclosed on every escalated turn rather than left to a
    // setting the person configured once and will not remember (ADR 0055).
    if reply.escalated {
        appended.push_str(
            "\n\n[deep] My local model could not answer that, so I asked the deep model \
             instead — this conversation left the box.",
        );
    }
    let reply_text = format!("{reply_text}{appended}");
    // `take_turn` is non-streaming, so deliver the final answer at once.
    // Send only what the person has not already seen.
    //
    // Diffing against everything streamed does not work: the model often writes a line
    // before calling a tool ("let me check the kitchen"), which reaches the person and is
    // then NOT part of the final answer — so the accumulated stream is no longer a prefix
    // of the reply, and the whole answer would arrive a second time.
    //
    // The signal is simpler than a diff. If the answering round produced text, that text
    // is exactly what streamed, and only the notes appended after it are new. If it
    // produced nothing, whatever stands in for it was never streamed at all.
    if answered_in_its_own_words {
        if !appended.is_empty() {
            on_token(&appended);
        }
    } else {
        on_token(&reply_text);
    }
    let butler_msg = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        &reply_text,
        clock.now(),
    )?;
    chat.append(&butler_msg)?;

    // Understanding is KEPT (ADR 0052) — but only on a CONVERSATION turn. On an action
    // turn (a tool ran, failed, or was blocked) we skip the extra model round: it's the
    // biggest latency saving on a slow model, and belief-forming matters far less when
    // the person was giving a command than when they were talking. On a pure chat turn,
    // a tools-OFF envelope pass forms beliefs (an empty `tools` context keeps the JSON
    // envelope, so understanding comes back without pulling the model into an action).
    let acted = activity.iter().any(|a| {
        a.starts_with("Used the ") || a.starts_with("Tried the ") || a.starts_with("Couldn't use ")
    });
    if !acted {
        let mut belief_ctx = context.clone();
        belief_ctx.tools = Vec::new();
        let formed = butler
            .respond(&history, &prefs, &belief_ctx)
            .map(|r| r.beliefs)
            .unwrap_or_default();
        record_formed_beliefs(beliefs, formed, ids, clock, &mut activity)?;
    }

    Ok((butler_msg, activity))
}
