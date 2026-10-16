use super::*;

// ---------------------------------------------------------------------------
// Notions — what Endora is still thinking about (ADR 0057).
//
// Everything here is pure and deterministic. A language model proposes the wording of a
// notion; nothing in this section trusts it. The model's output is a claim *and a set of
// pointers*, and only the pointers that turn out to name a real record — one that actually
// speaks to what was claimed — survive to be stored.
//
// This is the same instinct as `facts_behind` further up, for the same reason: ADR 0053
// measured this model following a direct instruction about verification roughly one run in
// three, so a guarantee written into a prompt is not a guarantee.
// ---------------------------------------------------------------------------

/// One record a pass may cite, with enough of its text to check a claim against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownRecord {
    /// Which store it came from.
    pub source: Source,
    /// How it is identified there.
    pub reference: String,
    /// What it says, in whatever words the store holds.
    pub text: String,
}

/// Everything a nightly pass is allowed to cite, gathered once from the stores.
///
/// Deliberately a **snapshot taken by the caller**, not a set of repository handles: the
/// checking is then pure, so the rule that decides whether Endora may believe something about
/// the person is testable without a database, and cannot quietly start reading somewhere new.
#[derive(Debug, Clone, Default)]
pub struct TheRecord {
    entries: Vec<KnownRecord>,
}

impl TheRecord {
    /// Builds a record set from what the caller read out of the stores.
    #[must_use]
    pub fn of(entries: Vec<KnownRecord>) -> Self {
        Self { entries }
    }

    /// What a cited record says, or `None` if there is no such record.
    #[must_use]
    pub fn text_of(&self, source: Source, reference: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.source == source && e.reference == reference)
            .map(|e| e.text.as_str())
    }

    /// Everything in the set.
    #[must_use]
    pub fn entries(&self) -> &[KnownRecord] {
        &self.entries
    }
}

/// What a pass proposed, before anything has checked it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProposedNotion {
    /// What the model thinks might be true about the person.
    pub statement: String,
    /// The records it says put the thought there.
    pub citations: Vec<(Source, String)>,
    /// What it says would settle the question.
    pub settles_when: String,
}

/// Whether a record's text actually speaks to a statement.
///
/// Existence alone is far too weak a test. A model handed a list of ids can cite them
/// perfectly while claiming something the records say nothing about, and the citations would
/// all resolve — so the notion would be stored, fully sourced and entirely invented.
///
/// So overlap is required too: **two content words in common**, or — where the record is
/// shorter than that — all of the words it has. The second clause is what lets a calendar
/// entry reading `Gym` support a notion about the gym, which a flat two-word rule would
/// refuse for having too little to say.
///
/// This does not establish that the record *supports* the claim, and nothing deterministic
/// could. It establishes that the claim was made about the record rather than beside it,
/// which is the difference between weak evidence and none.
#[must_use]
pub fn speaks_to(statement: &str, text: &str) -> bool {
    let claimed = keywords(statement);
    let said = keywords(text);
    if said.is_empty() {
        return false;
    }
    let shared = said.iter().filter(|w| claimed.contains(w)).count();
    shared >= said.len().min(2)
}

/// Keeps only the citations that name a record which exists **and** speaks to the statement.
///
/// The heart of ADR 0057. A citation that fails either test is not weak evidence to be scored
/// down — it is discarded, and a proposal left with nothing cannot become a notion at all.
#[must_use]
pub fn evidence_that_holds_up(proposed: &ProposedNotion, record: &TheRecord) -> Vec<Citation> {
    let mut kept = Vec::new();
    for (source, reference) in &proposed.citations {
        let Some(text) = record.text_of(*source, reference) else {
            continue; // names nothing that exists
        };
        if !speaks_to(&proposed.statement, text) {
            continue; // exists, but says nothing about this
        }
        if let Ok(citation) = Citation::new(*source, reference) {
            kept.push(citation);
        }
    }
    kept
}

/// Forms a notion from a proposal, or `None` if the record does not bear it out.
///
/// `None` is the expected outcome most nights, and that is the design working rather than
/// failing: a pass that proposes three things and stores none has correctly declined to
/// write three unfounded statements about the person.
#[must_use]
pub fn form_a_notion(
    id: NotionId,
    proposed: &ProposedNotion,
    record: &TheRecord,
    now: Timestamp,
) -> Option<Notion> {
    let held_up = evidence_that_holds_up(proposed, record);
    if held_up.is_empty() {
        return None;
    }
    Notion::new(
        id,
        &proposed.statement,
        held_up,
        &proposed.settles_when,
        now,
    )
    .ok()
}

/// New evidence for a notion already open — records that speak to it and are not already
/// cited.
///
/// Advancing a notion needs **no model at all**: the statement is fixed, so whether a record
/// speaks to it is the same arithmetic used to form it. That is what makes maturity mean
/// something. A notion cannot be talked into being believed; it has to keep being met by
/// records that arrive later, and a pass re-reading the same week finds nothing new because
/// the domain refuses evidence it already holds.
#[must_use]
pub fn new_support_for(notion: &Notion, record: &TheRecord) -> Vec<Citation> {
    record
        .entries()
        .iter()
        .filter(|e| speaks_to(notion.statement(), &e.text))
        .filter_map(|e| Citation::new(e.source, &e.reference).ok())
        .filter(|c| !notion.citations().contains(c))
        .collect()
}

/// Reads notions out of whatever the model wrote.
///
/// The shape is `NOTION: statement || source:reference, ... || what would settle it`, one per
/// line. Anything else on the line is ignored, and a line that does not parse yields nothing.
///
/// **Every failure here degrades to "no notion",** which is what makes handing this job to a
/// weak local model safe at all. A mangled line, a missing section, an invented source word,
/// a citation to a record that does not exist — each of them ends with Endora having formed
/// no thought, never with a wrong thought stored. The only way through is a well-formed line
/// whose citations then survive [`evidence_that_holds_up`].
#[must_use]
pub fn notions_proposed_in(text: &str) -> Vec<ProposedNotion> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches(['-', '*', ' ']);
        let Some(rest) = line
            .strip_prefix("NOTION:")
            .or_else(|| line.strip_prefix("notion:"))
            .or_else(|| line.strip_prefix("Notion:"))
        else {
            continue;
        };
        let mut parts = rest.split("||");
        let Some(statement) = parts.next().map(str::trim) else {
            continue;
        };
        if statement.is_empty() {
            continue;
        }
        let citations: Vec<(Source, String)> = parts
            .next()
            .unwrap_or_default()
            .split(',')
            .filter_map(|c| {
                let (source, reference) = c.trim().split_once(':')?;
                let source = Source::from_name(source.trim().to_lowercase().as_str()).ok()?;
                let reference = reference.trim();
                (!reference.is_empty()).then(|| (source, reference.to_owned()))
            })
            .collect();
        if citations.is_empty() {
            continue; // nothing to check it against
        }
        out.push(ProposedNotion {
            statement: statement.to_owned(),
            citations,
            settles_when: parts.next().unwrap_or_default().trim().to_owned(),
        });
    }
    out
}

/// What a matured notion becomes.
///
/// A [`BeliefKind::Pattern`] at [`Confidence::Medium`], always. A notion matures by being met
/// repeatedly by the record, which is the definition of a pattern rather than a stated value
/// or preference — and three records is real evidence but it is not the person saying so, so
/// the top of the scale stays reserved for what they confirm themselves (ADR 0052).
pub(super) fn what_a_matured_notion_becomes(
    notion: &Notion,
    id: BeliefId,
    now: Timestamp,
) -> Option<Belief> {
    let chain: Vec<String> = notion
        .citations()
        .iter()
        .map(|c| format!("{}:{}", c.source().name(), c.reference()))
        .collect();
    Belief::new(
        id,
        notion.statement(),
        BeliefKind::Pattern,
        Confidence::Medium,
        // The chain that produced it, so the reasoning is visible exactly where it pays off
        // — on the card the person can correct (ADR 0057).
        &format!("noticed over time, from {}", chain.join(", ")),
        now,
    )
    .ok()
}

/// How far back a pass looks for records to think about.
///
/// A fortnight, matching how long a notion may go unsupported: the window a thought can be
/// built from and the window it can die in are the same span, so a notion cannot be starved by
/// a pass that simply could not see far enough.
pub(super) const AS_FAR_BACK_AS_A_NOTION_LIVES_MS: i64 = 14 * 24 * 60 * 60 * 1_000;

/// Everything a pass is allowed to cite, read out of the stores.
///
/// Bounded by [`AS_FAR_BACK_AS_A_NOTION_LIVES_MS`] rather than by count, because the question
/// is what has happened lately and not how busy it has been.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn what_there_is_to_go_on(
    chat: &impl ChatRepository,
    outcomes: &impl OutcomeRepository,
    beliefs: &impl BeliefRepository,
    readings: Vec<(String, String)>,
    // What the watch loop has recorded moving (ADR 0059/0063) — the sensing the thinking
    // could not reach. Notions ran at zero for two days after shipping because the night
    // pass could cite an entity's state *now* and nothing about what had changed.
    transitions: &[endora_capabilities::Transition],
    theirs: &[String],
    now: Timestamp,
) -> Result<TheRecord, AppError> {
    let since = now.unix_millis() - AS_FAR_BACK_AS_A_NOTION_LIVES_MS;
    let mut entries = Vec::new();

    for message in chat.list()? {
        // Only what the person said. Endora's own words are not evidence about them — the
        // sharpest violation ADR 0052 found was a belief formed from Endora's own conduct,
        // and a pass citing its own replies would rebuild exactly that loop with more steps.
        if message.role() != MessageRole::User || message.at().unix_millis() < since {
            continue;
        }
        entries.push(KnownRecord {
            source: Source::Message,
            reference: message.id().value().to_string(),
            text: message.text().to_owned(),
        });
    }

    for outcome in outcomes.list()? {
        if outcome.at().unix_millis() < since {
            continue;
        }
        entries.push(KnownRecord {
            source: Source::Outcome,
            reference: outcome.id().value().to_string(),
            text: format!(
                "{} {} — {}",
                outcome.capability(),
                outcome.input(),
                outcome.observation().unwrap_or(outcome.claim())
            ),
        });
    }

    for belief in beliefs.list()? {
        entries.push(KnownRecord {
            source: Source::Belief,
            reference: belief.id().value().to_string(),
            text: belief.statement().to_owned(),
        });
    }

    entries.extend(readings_as_records(readings, theirs));
    entries.extend(transitions_as_records(transitions, theirs));

    Ok(TheRecord::of(entries))
}

/// Turns what moved into records a notion may cite (ADR 0063 closing 0057's loop).
///
/// Attribution is the same rule as a plain reading: the key names its entity after the
/// `server::` namespace, and only an entity the person's own service lists as theirs is
/// [`Source::Personal`] — everything else is the house, which corroborates and never
/// carries. References carry the timestamp so three changes of one thing are three
/// citations, not one said thrice: maturity is a count of *distinct* resolutions.
#[must_use]
pub fn transitions_as_records(
    transitions: &[endora_capabilities::Transition],
    theirs: &[String],
) -> Vec<KnownRecord> {
    transitions
        .iter()
        .map(|t| {
            let entity = t.key.split_once("::").map_or(t.key.as_str(), |(_, e)| e);
            KnownRecord {
                source: if theirs.iter().any(|mine| mine == entity) {
                    Source::Personal
                } else {
                    Source::Reading
                },
                reference: format!("{}@{}", t.key, t.at_ms),
                text: format!("{entity} went from {} to {}", t.from, t.to),
            }
        })
        .collect()
}

/// Turns a reading into a record, marking the ones that are **the person's own**.
///
/// The distinction is the whole of attribution (ADR 0057). A hallway light and a family
/// calendar belong to a household that has other people in it; a phone in the person's pocket
/// does not, and only the second may ever carry a notion into a belief about them.
///
/// `theirs` comes from the service, which holds the mapping because the person set it up. When
/// it is empty — nobody configured, or several people and no way to tell which one Endora
/// serves — **everything is the house**, which is the direction that fails safely.
#[must_use]
pub fn readings_as_records(readings: Vec<(String, String)>, theirs: &[String]) -> Vec<KnownRecord> {
    readings
        .into_iter()
        .map(|(entity, state)| KnownRecord {
            source: if theirs.contains(&entity) {
                Source::Personal
            } else {
                Source::Reading
            },
            text: format!("{entity} {state}"),
            reference: entity,
        })
        .collect()
}

/// Asks for notions, handing over the record to cite from.
///
/// The model's whole job is the wording. It cannot store anything, its citations are checked
/// against the same list it was given, and a reply that parses to nothing leaves Endora having
/// thought nothing — so this is a suggestion box, not a decision (ADR 0051).
///
/// The records go over as `source:reference` pairs because an id cannot be mis-matched, which
/// is the same reason ADR 0054 prefers a service's own interface to a prose description of it.
#[must_use]
pub fn ask_for_notions(
    butler: &dyn Butler,
    record: &TheRecord,
    already_wondering: &[Notion],
    ids: &impl IdSource,
    clock: &impl Clock,
) -> Vec<ProposedNotion> {
    let listing: Vec<String> = record
        .entries()
        .iter()
        .map(|e| {
            format!(
                "{}:{} — {}",
                e.source.name(),
                e.reference,
                e.text.chars().take(160).collect::<String>()
            )
        })
        .collect();
    if listing.is_empty() {
        return Vec::new();
    }
    let held: String = if already_wondering.is_empty() {
        "nothing yet".to_owned()
    } else {
        already_wondering
            .iter()
            .map(|n| format!("- {}", n.statement()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let disguise = crate::egress::personal_values_in(&ButlerContext {
        present: record.entries().iter().map(|e| e.text.clone()).collect(),
        ..ButlerContext::default()
    });
    let ask = format!(
        "Here are records from the last fortnight:\n{}\n\nYou are already wondering \
         about:\n{held}\n\nIs there a pattern here worth watching — something that might be \
         true about them but is not settled yet? Answer with at most one line, in exactly \
         this shape, or with nothing at all if there is no pattern:\n\n\
         NOTION: <what you suspect> || <source:reference>, <source:reference> || <what would \
         settle it>\n\nCite only records from the list above, and only ones that actually \
         support what you suspect. Do not repeat something you are already wondering about.",
        disguise.hide(&listing.join("\n"))
    );
    let Ok(message) = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        &ask,
        clock.now(),
    ) else {
        return Vec::new();
    };
    let bare = ButlerContext {
        now: clock_line(clock),
        ..ButlerContext::default()
    };
    let Ok(reply) = butler.respond(&[message], &[], &bare) else {
        return Vec::new();
    };
    notions_proposed_in(&disguise.restore(&reply.text))
}

/// The thinking part of the night (ADR 0057).
///
/// Runs in one order, and the order is the design:
///
/// 1. **advance** what is open, from records that arrived since — no model involved;
/// 2. **promote** whatever has now earned belief;
/// 3. **let go** of what nothing has spoken to in a fortnight;
/// 4. **form** at most one new notion, displacing the weakest if the cap is full.
///
/// Forming comes last so that a new speculation can never push out something that was about
/// to mature on this very pass.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn think_about_the_day(
    notions: &impl NotionRepository,
    beliefs: &impl BeliefRepository,
    record: &TheRecord,
    proposed: &[ProposedNotion],
    ids: &impl IdSource,
    clock: &impl Clock,
    activity: &mut Vec<String>,
) -> Result<(), AppError> {
    let now = clock.now();

    // 1 + 2. Advance, then promote.
    for mut notion in notions.open()? {
        let mut moved = false;
        for citation in new_support_for(&notion, record) {
            moved |= notion.support(citation, now);
        }
        if notion.is_ready_to_believe() {
            if let Some(belief) =
                what_a_matured_notion_becomes(&notion, BeliefId::new(ids.new_id()), now)
            {
                beliefs.save(&belief)?;
                notion.mature();
                notions.save(&notion)?;
                activity.push(format!(
                    "Something I'd been watching added up: {}",
                    notion.statement()
                ));
                continue;
            }
        }
        if moved {
            notions.save(&notion)?;
        }
    }

    // 3. Let go of what has gone quiet. Nothing is dismissed by hand and nothing accumulates.
    for mut notion in notions.open()? {
        if !notion.has_gone_quiet(now) {
            continue;
        }
        notion.die();
        notions.save(&notion)?;
        activity.push(format!(
            "Stopped wondering whether {} — nothing came of it",
            notion.statement()
        ));
    }

    // 4. Form at most one. One, because a pass that files five speculations about somebody in
    // a night is the thing this design exists to avoid, and because the cap should be reached
    // by thinking over a week rather than in a single evening.
    for proposal in proposed {
        let Some(formed) = form_a_notion(NotionId::new(ids.new_id()), proposal, record, now) else {
            continue; // the record does not bear it out
        };
        let open = notions.open()?;
        // Against what it is already wondering **and** what it already believes. The second
        // half is not belt-and-braces: a notion that matured earlier in this very pass is no
        // longer open, so without it the pass could promote a thought to a belief and then
        // immediately start wondering the same thing again — the duplicate-on-one-screen
        // failure ADR 0052 was written about, arriving by a new route.
        let settled = beliefs.list()?;
        let already_held = open
            .iter()
            .map(Notion::statement)
            .chain(settled.iter().map(Belief::statement))
            .any(|held| says_the_same_thing(held, formed.statement()));
        if already_held {
            continue;
        }
        if let Some(displaced) = make_way_for_a_new_one(&open) {
            if let Some(mut weakest) = open.into_iter().find(|n| n.id() == displaced) {
                weakest.die();
                notions.save(&weakest)?;
            }
        }
        notions.save(&formed)?;
        activity.push(format!("Started wondering whether {}", formed.statement()));
        return Ok(());
    }

    // Nothing was formed, and **which kind of nothing matters**.
    //
    // Every failure on this path is silent by design — a proposal the record will not bear out
    // is discarded rather than stored, which is the whole guarantee. But that left no way to
    // tell a model that **said nothing usable** from a model whose suggestion was **refused**,
    // and the two mean opposite things: the first is a model that cannot hit the shape and
    // will never form a notion at all, the second is the guarantee working exactly as
    // intended. The first live pass produced precisely that ambiguity, and there was nothing
    // to look at.
    //
    // ADR 0053 already settled the principle for every other path here: *the interface
    // discloses, whatever the model said*. A turn that changed nothing says so. This one now
    // does too.
    if proposed.is_empty() {
        activity.push(format!(
            "Looked over {} things from the last fortnight and found nothing worth watching",
            record.entries().len()
        ));
    } else {
        activity.push(format!(
            "Turned over {} idea{} about you — none of it held up against the record",
            proposed.len(),
            if proposed.len() == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

/// The stored nightly-loop schedule, defaulting to **off** (ADR 0051).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn nightly_loop_schedule(
    schedules: &impl NightlyLoopScheduleRepository,
) -> Result<NightlyLoopSchedule, AppError> {
    Ok(schedules
        .get()?
        .unwrap_or_else(NightlyLoopSchedule::disabled_default))
}

/// Sets the nightly-loop cadence (on/off + UTC hour), preserving `last_at` so
/// toggling it doesn't re-fire the same night (ADR 0051).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn set_nightly_loop_schedule(
    schedules: &impl NightlyLoopScheduleRepository,
    enabled: bool,
    hour_utc: u8,
) -> Result<NightlyLoopSchedule, AppError> {
    let current = schedules
        .get()?
        .unwrap_or_else(NightlyLoopSchedule::disabled_default);
    let schedule = NightlyLoopSchedule {
        enabled,
        hour_utc: hour_utc.min(23),
        last_at: current.last_at,
    };
    schedules.set(&schedule)?;
    Ok(schedule)
}

/// If the **nightly self-improvement loop** is due (enabled, the hour matches, it
/// hasn't run tonight), runs it and records that it fired (ADR 0051). Called by the
/// heartbeat at a quiet off-hour.
///
/// The loop stays entirely within the **reversible band**: it may run reversible
/// *information* skills to research a topic the person cares about (the *experiment*
/// step), reviews the recent conversation and Endora's current understanding, has
/// the butler **reflect** — forming and refining beliefs (saved like a normal
/// turn) — and leaves a short **overnight note**. It never runs a consequential or
/// irreversible skill — policy clears only reversible, autonomous ones — so there is
/// nothing here it could do that it couldn't undo. Returns the posted note (if any)
/// + activity, or `None` when not due / nothing to say.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure, or [`AppError::Model`] if the
/// butler errors.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn run_due_nightly_loop(
    chat: &impl ChatRepository,
    beliefs: &impl BeliefRepository,
    preferences: &impl PreferenceRepository,
    outcomes: &impl OutcomeRepository,
    specimens: &impl SpecimenRepository,
    intentions: &impl IntentionRepository,
    notions: &impl NotionRepository,
    // The fortnight's transitions, for the record the thinking cites from.
    transitions: &[endora_capabilities::Transition],
    schedules: &impl NightlyLoopScheduleRepository,
    capabilities: &dyn CapabilityRunner,
    // What the services say belongs to the person rather than the household (ADR 0057).
    theirs: &[String],
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    // Whether the record lane is on (`ENDORA_OWN_WORK`, ADR 0095). When it is, the lane asks
    // the stumpers again and looks into beliefs, so this loop stops doing both; when it is
    // not, this loop does what it always did, so there is never a time when neither does.
    lane_is_on: bool,
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let now = clock.now();
    let Some(mut schedule) = schedules.get()? else {
        return Ok(None);
    };
    if !schedule.is_due(now) {
        return Ok(None);
    }
    // Mark fired first, so a slow reflection can't double-run on the next tick.
    schedule.last_at = now;
    schedules.set(&schedule)?;

    let mut history = chat.list()?;
    let prefs = preferences.list_all()?;
    let mut activity: Vec<String> = Vec::new();
    // The reward signal, nightly (loop spine). Derived from read-back, never from the
    // model's account of itself; recorded where the person reads and the butler's own
    // own-activity skill can reach it from any turn.
    if let Some(score) = scorecard(&outcomes.list()?) {
        activity.push(score);
    }

    // REPLAY ONE STUMPER (ADR 0075). An ask that once ended at the honesty valve is
    // re-asked against tonight's machinery — under the same reversible-only runner
    // as the rest of the night, so a replay can gather and never actuate. Passing
    // retires the specimen: the record notices the fleet getting better without the
    // person having to ask again. One per night, oldest first — unhurried, like the
    // rest — and judged by the same deterministic verdict that filed it.
    let stumper = if lane_is_on {
        None
    } else {
        specimens.open_specimens()?.into_iter().next()
    };
    if let Some(stumper) = stumper {
        let ask = [ChatMessage::new(
            MessageId::new(ids.new_id()),
            MessageRole::User,
            &stumper.asked,
            clock.now(),
        )?];
        // Unmotivated, and nothing appended to the chat: a replay is the butler
        // examining itself, not a conversation.
        let unmotivated = OutcomeSink::unmotivated(outcomes);
        let replayed = run_tool_turn(
            butler,
            capabilities,
            audit,
            &unmotivated,
            ids,
            clock,
            &ask,
            &|_id| Vec::new(),
            &prefs,
            context,
            CHAT_TOOL_ROUNDS,
            &mut |_step| {},
            &mut |_token: &str| {},
            &mut Vec::new(),
            &mut Vec::new(),
        );
        let passed = replayed
            .as_ref()
            .map(|reply| specimen_verdict(reply, context).is_none())
            .unwrap_or(false);
        let landed = specimens.record_replay(&stumper.id, passed, clock.now().unix_millis())?;
        // Three outcomes, not two. Giving up is the one worth naming: it is the
        // night the record stops being "still trying" and becomes evidence that
        // nothing here can answer this — which ADR 0075 said the trail would carry,
        // and which ADR 0071 reads as a proven gap.
        activity.push(format!(
            "{} \"{}\"",
            match landed {
                endora_understanding::SpecimenState::Answered =>
                    "A question that once stumped me answers now:",
                endora_understanding::SpecimenState::GaveUp =>
                    "I've stopped re-asking this one — nothing I have can answer it:",
                endora_understanding::SpecimenState::Open => "Replayed a stumper; it still fails:",
            },
            first_sentence_of(&stumper.asked)
        ));
    }

    // AGENTIC overnight review (ADR 0056/0024): name a focus the person cares about
    // and let the butler reach for WHATEVER skills help look into it — its choice,
    // not a fixed script — then review the day and reflect, all in one grounded,
    // policy-gated pass (reversible + autonomous only; it drafts and forms beliefs
    // but does nothing it couldn't undo). The final reply IS the reflection.
    // Retire what's over before deciding anything, so a spent or stale thread can't
    // occupy the one active slot (ADR 0052).
    if let Some(why) = retire_finished_intention(intentions, clock)? {
        activity.push(format!("Stopped working on something — {why}"));
    }
    // Continue what Endora is already pursuing, or take something up. This is the
    // change ADR 0052 is for: seven nights on one thing, not seven unrelated evenings.
    // With the lane on, the lane looks into the belief and this loop keeps no intention of its
    // own; one that was already active is still retired above when it is spent or stale.
    let intention = match intentions.active()? {
        _ if lane_is_on => None,
        Some(existing) => Some(existing),
        None => take_up_an_intention(intentions, beliefs, ids, clock, &mut activity)?,
    };
    // Overnight work traces to the belief that prompted it (ADR 0053/0036).
    let actions = OutcomeSink::motivated_by(
        outcomes,
        intention.as_ref().and_then(Intention::motivating_belief),
    );
    let instruction = match intention.as_ref() {
        // Picked it up tonight — nothing found yet, so there is nothing to resume.
        Some(i) if i.note().is_empty() => format!(
            "It's the quiet overnight hour and they're away. You're taking up \"{}\" — \
             something they care about — so start looking into it, reaching for whatever \
             skills would help. Then review our recent conversation and what you understand \
             about them: note privately what you've learned or grown more sure of (as \
             beliefs), and leave a short, warm note they'll see in the morning — what you \
             looked into, what you noticed, and what you'll keep an eye on. Add nothing you \
             don't actually know.",
            i.statement()
        ),
        // Already under way: hand back its own account of where it got to, so tonight
        // continues the thread instead of starting it again.
        Some(i) => format!(
            "It's the quiet overnight hour and they're away. You're already looking into \
             \"{}\" — this is night {} of it. Here is what you found last time, in your own \
             words:\n\n{}\n\nCarry on from there rather than starting over: what's the next \
             thing worth checking? Reach for whatever skills would help. Then review our \
             recent conversation and what you understand about them, note privately what \
             you've learned (as beliefs), and leave a short, warm note they'll see in the \
             morning. Add nothing you don't actually know.",
            i.statement(),
            i.steps_taken() + 1,
            i.note()
        ),
        None => "It's the quiet overnight hour and they're away. Review our recent \
             conversation and what you understand about them: note privately what you've \
             learned or grown more sure of (as beliefs), and leave a short, warm note \
             they'll see in the morning — what you noticed and what you'll keep an eye on. \
             If there's genuinely nothing new, keep the note brief or say so plainly."
            .to_owned(),
    };
    history.push(ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        &instruction,
        now,
    )?);
    let review_ctx = ButlerContext {
        now: clock_line(clock),
        ..context.clone()
    };
    let reply = run_tool_turn(
        butler,
        capabilities,
        audit,
        &actions,
        ids,
        clock,
        &history,
        &|_id| Vec::new(),
        &prefs,
        &review_ctx,
        NIGHTLY_TOOL_ROUNDS,
        &mut |_step| {},
        &mut |_token: &str| {},
        &mut activity,
        &mut Vec::new(),
    )?;

    // Judged before the beliefs are moved out of the reply, and before anything is stored:
    // a night that produced no answer must not leave a trace as though it had.
    let worth_recording = !not_an_answer(&reply, &review_ctx);
    // Reflect: persist the understanding it formed (same path as a chat turn).
    record_formed_beliefs(beliefs, reply.beliefs, ids, clock, &mut activity)?;

    // Remember where tonight got to, in the butler's own words, so the next night can
    // pick the thread up (ADR 0052). Prose in, prose out — no state machine for the
    // model to maintain, and nothing here can corrupt if it writes something odd.
    if let Some(mut intention) = intention {
        let note = reply.text.trim();
        // A night that produced no answer is not a night's work. Endora's one long-running
        // intention — "you want to run more often" — reached five of its seven steps with
        // its stored progress reading *"Sorry, I couldn't reach my language model just
        // now."* An apology is non-empty, so it counted as a step, was written down as
        // what happened, and was handed to the next night as the thread to pick up.
        //
        // Two things were being spent on nothing: the step budget that makes an intention
        // retire itself, and the only record of what Endora has been doing about the thing
        // it decided to care about. The same predicate every other path already uses
        // settles it (ADR 0052).
        if !note.is_empty() && worth_recording {
            intention.progress(note, now);
            intentions.save(&intention)?;
            activity.push(format!(
                "Made progress on \"{}\" (night {})",
                intention.statement(),
                intention.steps_taken()
            ));
        }
    }

    // Think: carry the unfinished thoughts forward (ADR 0057). This is the part of the night
    // that is not about tonight — it advances what the record has since spoken to, promotes
    // whatever has now been met enough times to believe, lets go of what nothing came of, and
    // wonders at most one new thing.
    //
    // It runs whatever the model did above, because three of its four steps need no model at
    // all: a notion maturing is arithmetic over records that arrived on their own, and a night
    // where the language model was unreachable is still a night in which the person's calendar
    // and the house went on happening.
    let record = what_there_is_to_go_on(
        chat,
        outcomes,
        beliefs,
        capabilities.current_states(),
        transitions,
        theirs,
        now,
    )?;
    let proposed = if worth_recording {
        ask_for_notions(butler, &record, &notions.open()?, ids, clock)
    } else {
        // A degraded turn produced no answer, so it has no suggestion to make either. Asking
        // again here would spend a second call to a model that just failed, and any line it
        // did return would be exactly the kind of thing that should never become a stored
        // statement about somebody (ADR 0056).
        Vec::new()
    };
    think_about_the_day(
        notions,
        beliefs,
        &record,
        &proposed,
        ids,
        clock,
        &mut activity,
    )?;

    // Forget: age out beliefs nothing has reinforced in a long time (ADR 0052).
    // Understanding is a living model, so the overnight review is where it *loses*
    // things as well as gains them.
    for statement in expire_faded_beliefs(beliefs, clock)? {
        activity.push(format!("Let go of a stale belief: {statement}"));
    }

    // Surface: leave the overnight note, if it wrote one. If it only refined
    // beliefs and had nothing worth saying, don't post a chat message — the beliefs
    // are already saved and reviewable in Understanding; we just stay quiet.
    let text = reply.text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let message = post_butler_message(chat, ids, clock, text)?;
    activity.push("Left an overnight note".to_owned());
    Ok(Some((message, activity)))
}

/// The topic the nightly loop researches, taken from what Endora actually
/// **understands** about the person (ADR 0052): the intent it is most sure of, since
/// intent is the slow-changing thing worth looking into overnight. Failing that, the
/// strongest belief of any kind. `None` when Endora understands nothing yet, so the
/// loop simply reflects without researching.
///
/// Confidence is the ranking, deliberately: researching a tentative guess spends the
/// night on something Endora may be wrong about.
/// Retires the active intention if it is spent or stale, freeing the one slot
/// (ADR 0052). Returns why, in plain words, for the activity trail.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub(super) fn retire_finished_intention(
    intentions: &impl IntentionRepository,
    clock: &impl Clock,
) -> Result<Option<String>, AppError> {
    let Some(mut active) = intentions.active()? else {
        return Ok(None);
    };
    let Some(why) = active.retire_if_over(clock.now()) else {
        return Ok(None);
    };
    let reason = format!("{why} (\"{}\")", active.statement());
    intentions.save(&active)?;
    Ok(Some(reason))
}

/// Takes up something to pursue, from the belief Endora is most sure about (ADR 0052).
///
/// Only ever called with no active intention, so the one-at-a-time rule holds by
/// construction rather than by a check that could be forgotten. `None` when there is
/// nothing understood well enough to pursue — a new Endora simply has no thread yet.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails, or [`AppError::Domain`] if the
/// belief's statement somehow does not make a valid intention.
pub(super) fn take_up_an_intention(
    intentions: &impl IntentionRepository,
    beliefs: &impl BeliefRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    activity: &mut Vec<String>,
) -> Result<Option<Intention>, AppError> {
    let Some((belief, statement)) = nightly_focus(beliefs, clock)? else {
        return Ok(None);
    };
    let intention = Intention::form(
        IntentionId::new(ids.new_id()),
        &statement,
        belief,
        clock.now(),
    )
    .map_err(AppError::Domain)?;
    intentions.save(&intention)?;
    activity.push(format!("Started looking into \"{statement}\""));
    Ok(Some(intention))
}

pub(super) fn nightly_focus(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Option<(BeliefId, String)>, AppError> {
    let active = understanding(beliefs, clock)?;
    let strongest = |kind: Option<crate::BeliefKind>| {
        active
            .iter()
            .filter(|b| kind.is_none_or(|k| b.kind() == k))
            .max_by_key(|b| match b.confidence() {
                crate::Confidence::High => 2,
                crate::Confidence::Medium => 1,
                crate::Confidence::Low => 0,
            })
            .map(|b| (b.id(), b.statement().to_owned()))
    };
    Ok(strongest(Some(crate::BeliefKind::Intent)).or_else(|| strongest(None)))
}

/// Asks again a question the butler once failed, as work in the record lane, and says what
/// became of it (ADR 0095). The nightly loop's "replay one stumper" step, moved here.
///
/// Judged by the same deterministic verdict that filed the specimen
/// ([`specimen_verdict`]), never by the model: a pass completes the goal and retires the
/// specimen, a failure ends the goal `Partial` and counts against the specimen, which gives
/// up after [`endora_understanding::REPLAYS_BEFORE_GIVING_UP`] of them. The specimen holds
/// the count, so an ask is not replayed more often than it ever was.
///
/// Held to the local model, like all of the lane; allowed the skills the nightly replay had,
/// because it is the person's own ask and a replay denied the skills it needs would always
/// fail and falsely prove that nothing can answer it. Nothing is appended to the chat: a
/// replay is the butler examining itself, not a conversation.
///
/// # Errors
/// [`AppError::Repository`] if the replay cannot be recorded. Then the goal is left as it
/// was and the worker tries again, rather than ending a goal whose result was not kept.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn replay_specimen(
    record: &crate::GoalRecord,
    specimen: &endora_understanding::Specimen,
    butler: &dyn Butler,
    capabilities: &dyn CapabilityRunner,
    audit: &dyn AuditLog,
    outcomes: &impl OutcomeRepository,
    specimens: &impl SpecimenRepository,
    preferences: &impl PreferenceRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
) -> Result<crate::GoalRecord, AppError> {
    let mut goal = record.goal().clone();
    if goal.status().terminal() {
        return Ok(record.clone());
    }
    let local_only = crate::record_lane::LocalOnly::new(butler);
    let prefs = preferences.list_all()?;
    let ask = [ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        &specimen.asked,
        clock.now(),
    )?];
    // Unmotivated: a replay is not caused by any belief.
    let unmotivated = OutcomeSink::unmotivated(outcomes);
    let replayed = run_tool_turn(
        &local_only,
        capabilities,
        audit,
        &unmotivated,
        ids,
        clock,
        &ask,
        &|_id| Vec::new(),
        &prefs,
        context,
        CHAT_TOOL_ROUNDS,
        &mut |_step| {},
        &mut |_token: &str| {},
        &mut Vec::new(),
        &mut Vec::new(),
    );
    let passed = replayed
        .as_ref()
        .map(|reply| specimen_verdict(reply, context).is_none())
        .unwrap_or(false);
    let landed = specimens.record_replay(&specimen.id, passed, clock.now().unix_millis())?;
    // Three outcomes, not two. Giving up is the one worth naming: it is the point at which
    // the record stops being "still trying" and becomes evidence that nothing here can
    // answer this, which is what ADR 0071 reads as a proven gap.
    let line = format!(
        "{} \"{}\"",
        match landed {
            endora_understanding::SpecimenState::Answered =>
                "A question that once stumped me answers now:",
            endora_understanding::SpecimenState::GaveUp =>
                "I've stopped re-asking this one — nothing I have can answer it:",
            endora_understanding::SpecimenState::Open => "Replayed a stumper; it still fails:",
        },
        first_sentence_of(&specimen.asked)
    );
    goal.start("replay", "ask it again");
    goal.commit("replay", &line);
    if passed {
        goal.apply(
            crate::GoalVerdict::Complete,
            "it passed the check that filed it",
        );
    } else {
        goal.apply(crate::GoalVerdict::Partial, "it still fails the check");
    }
    Ok(record.with_goal(goal, clock.now()))
}

/// The belief Endora is most sure of, as the lane's last source (ADR 0095): the same choice
/// the nightly loop made for its intention, by the same arithmetic. `None` when it understands
/// nothing well enough yet.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn belief_to_look_into(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Option<crate::own_work::Focus>, AppError> {
    Ok(
        nightly_focus(beliefs, clock)?.map(|(id, statement)| crate::own_work::Focus {
            id: id.value().to_string(),
            statement,
        }),
    )
}

/// The ids of the beliefs still held, in the form a goal's source records them, so that work
/// begun about one can tell the belief has faded or been dropped.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn beliefs_still_held(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Vec<String>, AppError> {
    Ok(understanding(beliefs, clock)?
        .iter()
        .map(|belief| belief.id().value().to_string())
        .collect())
}
