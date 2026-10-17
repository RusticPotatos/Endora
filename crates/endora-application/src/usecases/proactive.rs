use super::*;

/// Returns the whole conversation with the butler, oldest first.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn chat_history(chat: &impl ChatRepository) -> Result<Vec<ChatMessage>, AppError> {
    Ok(chat.list()?)
}

/// The sections of the person's **standing order** (ADR 0074), gathered by code.
///
/// Each section is a configured skill called with known-good arguments, its summary
/// included verbatim — "top three headlines" is enforced by the call, never asked of
/// the model. A skill that is not configured contributes nothing, so the brief
/// shrinks honestly; one that fails is noted in the activity trail and its section
/// is absent rather than apologised for every morning — the trail is the operator's,
/// the brief is the person's.
///
/// The mail rides along (headers only, through the house — ADR 0058/0064): who has
/// written and what about is exactly a morning question, and it was the one section
/// the person noticed missing on the first fully-configured brief.
/// One section of the brief: the heading a person reads it under, the plainer
/// fragment the overview line names it by, and the body text.
pub(super) struct BriefSection {
    heading: &'static str,
    named: &'static str,
    body: String,
}

pub(super) fn gathered_brief_sections(
    capabilities: &dyn CapabilityRunner,
    place: &str,
    activity: &mut Vec<String>,
) -> Vec<BriefSection> {
    const TOP_HEADLINES: u8 = 3;
    let configured: std::collections::HashSet<String> = capabilities
        .available()
        .into_iter()
        .filter(|s| s.configured)
        .map(|s| s.id)
        .collect();
    // (capability id, activity-trail label, heading, overview fragment, args)
    let mut asks: Vec<(&str, &str, &'static str, &'static str, String)> = vec![
        (
            "traffic",
            "drive",
            "The drive",
            "the drive",
            "{}".to_owned(),
        ),
        ("mail", "mail", "Mail", "the mail", "{}".to_owned()),
    ];
    // Sections that need a place are skipped without one — a brief must never guess
    // where "home" is (ADR 0065).
    if !place.trim().is_empty() {
        asks.insert(
            0,
            (
                "weather",
                "weather",
                "Weather",
                "the weather",
                serde_json::json!({ "location": place }).to_string(),
            ),
        );
        asks.push((
            "news",
            "news",
            "News",
            "the headlines",
            serde_json::json!({ "location": place, "count": TOP_HEADLINES }).to_string(),
        ));
        asks.push((
            "ticketed_events",
            "events",
            "Local events",
            "local events",
            serde_json::json!({ "city": place }).to_string(),
        ));
    }
    asks.push((
        "city_meetings",
        "city meetings",
        "City meetings",
        "city meetings",
        "{}".to_owned(),
    ));
    let mut sections = Vec::new();
    for (id, label, heading, named, args) in asks {
        if !configured.contains(id) {
            continue;
        }
        match capabilities.run(id, &args) {
            Ok(s) if !s.trim().is_empty() => {
                activity.push(format!("Fetched the {label}"));
                sections.push(BriefSection {
                    heading,
                    named,
                    body: s.trim().to_owned(),
                });
            }
            Ok(_) => {}
            Err(_) => activity.push(format!("Tried the {label}, but it failed")),
        }
    }
    sections
}

/// A one-line lead naming what the brief holds, so the person can stop reading
/// after a sentence if that is enough, or keep going for the rest. Plain text —
/// no markdown — because it is also what a truncated push notification shows
/// (`reach_out` takes the first sentence), where a stray `**` would be noise.
///
/// Live: a brief with no pacing at all — one unbroken run of bullet points, an
/// entity-id dump in the middle of it, two dense sections run together with no
/// gap — was unreadable end to end, and there was nowhere to look to get the gist
/// without reading everything. This is that gist.
pub(super) fn brief_overview(named: &[&str]) -> Option<String> {
    if named.is_empty() {
        return None;
    }
    let joined = match named {
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => unreachable!("checked non-empty above"),
    };
    Some(format!(
        "Today's brief covers {joined} — the details are below."
    ))
}

/// Everything a brief has to say, assembled deterministically (ADR 0074): what
/// Endora already holds (presence, today's calendar — via
/// [`whats_worth_saying_this_morning`]) plus the standing-order sections gathered
/// fresh by [`gathered_brief_sections`]. `None` means there is genuinely nothing —
/// no brief, rather than a cheerful nothing.
pub fn assembled_brief_facts(
    capabilities: &dyn CapabilityRunner,
    context: &ButlerContext,
    activity: &mut Vec<String>,
) -> Option<String> {
    // Each block gets its own heading and stands apart from its neighbours by a
    // blank line — a single run of bullets with no pacing was the thing that made
    // this unreadable in the first place, and a blank line is the one separator
    // that survives every surface the text ends up on (the console, a push
    // notification, plain mail).
    let mut blocks: Vec<String> = Vec::new();
    let mut named: Vec<&str> = Vec::new();
    if let Some(known) = whats_worth_saying_this_morning(context) {
        named.push("what's around the house");
        blocks.push(format!("**Around the house**\n{known}"));
    }
    for section in gathered_brief_sections(capabilities, &context.where_they_are, activity) {
        named.push(section.named);
        blocks.push(format!("**{}**\n{}", section.heading, section.body));
    }
    if blocks.is_empty() {
        return None;
    }
    let mut out = String::new();
    if let Some(overview) = brief_overview(&named) {
        out.push_str(&overview);
        out.push_str("\n\n");
    }
    out.push_str(&blocks.join("\n\n"));
    Some(out)
}

/// The standing questions (ADR 0076): asks a person makes routinely whose complete
/// answer is derivable by code. Policy routes them; the model is never consulted.
/// This is THE one routing point — the brief's bespoke pre-route (ADR 0074) folded
/// in here the day a second question earned the mechanism its name. Every entry
/// falls through to the ordinary turn when it cannot assemble its answer: broken
/// routing degrades to a conversation, never to silence. `private` is a 🔒 ask: its answer
/// is never worded outside the house (ADR 0105).
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub(super) fn standing_answer(
    text: &str,
    private: bool,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    ids: &impl IdSource,
    clock: &impl Clock,
    on_token: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(ButlerStep),
    activity: &mut Vec<String>,
) -> Option<String> {
    if asked_for_their_brief(text) {
        if let Some(answer) = brief_on_request(
            private,
            capabilities,
            butler,
            ids,
            clock,
            on_token,
            on_step,
            activity,
        ) {
            return Some(answer);
        }
    }
    if let Some(kind) = asked_about_a_kind(text, &capabilities.current_states()) {
        if let Some(answer) = kind_status_answer(&kind, &capabilities.current_states()) {
            activity.push("Answered from the house's own reading".to_owned());
            on_token(&answer);
            return Some(answer);
        }
    }
    None
}

/// Whether the person is asking for their brief — the standing order of ADR 0074.
///
/// The determiner+noun bigram, never the bare word: "keep it brief" and "a brief
/// pause" are adjectives, and answering a style request with a weather report would
/// be worse than missing. "a brief rundown" does match — and a person asking for a
/// brief rundown is asking for exactly what the standing order holds, so that false
/// positive serves them. "brief me" is the verb form of the same request.
pub(super) fn asked_for_their_brief(text: &str) -> bool {
    // "today's" splits into `today` + `s` on the word boundary; the possessive
    // fragment would break the bigram, so it is dropped before matching.
    let words: Vec<String> = words_of(text).into_iter().filter(|w| w != "s").collect();
    words.windows(2).any(|pair| {
        let noun = matches!(
            pair[0].as_str(),
            "my" | "the" | "a" | "morning" | "daily" | "afternoon" | "evening" | "today" | "todays"
        ) && pair[1] == "brief";
        let verb = pair[0] == "brief" && pair[1] == "me";
        noun || verb
    })
}

/// Runs the brief standing order for a chat ask (ADR 0074): the `brief` skill
/// assembles the sections, the deep model words them exactly as the scheduled brief
/// is worded, and the model never routes. `None` — fall through to the ordinary
/// turn — when the skill is absent, not configured, or failed: a broken assembly
/// must degrade to a conversation, never to silence. Asked `private`ly, the facts post
/// as themselves (ADR 0105).
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub(super) fn brief_on_request(
    private: bool,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    ids: &impl IdSource,
    clock: &impl Clock,
    on_token: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(ButlerStep),
    activity: &mut Vec<String>,
) -> Option<String> {
    const THE_BRIEF: &str = "brief";
    if !capabilities
        .available()
        .iter()
        .any(|s| s.id == THE_BRIEF && s.configured)
    {
        return None;
    }
    let step = |status: StepStatus, output: Option<String>| ButlerStep {
        skill: THE_BRIEF.to_owned(),
        status,
        label: "Assembling your brief".to_owned(),
        output,
    };
    on_step(step(StepStatus::Running, None));
    match capabilities.run(THE_BRIEF, "{}") {
        Ok(facts) if !facts.trim().is_empty() => {
            on_step(step(StepStatus::Done, Some(facts.clone())));
            activity.push("Assembled your brief".to_owned());
            // Asked privately, the facts post as themselves: wording them would send them to
            // the deep model, and a private turn never leaves the house (ADR 0105).
            let text = if private {
                facts.trim().to_owned()
            } else {
                worded_brief(butler, facts.trim(), ids, clock)
            };
            // Streamed as one chunk: the tail of the turn treats a reply with text
            // as already-delivered, and this one never went through the model.
            on_token(&text);
            Some(text)
        }
        Ok(_) => {
            on_step(step(StepStatus::Done, None));
            None
        }
        Err(e) => {
            eprintln!("turn: the brief assembly failed — {e}");
            on_step(step(StepStatus::Failed, Some(format!("error: {e}"))));
            activity.push("Tried to assemble your brief, but it failed".to_owned());
            None
        }
    }
}

/// Whether the ask is a STATUS question about a kind of thing the house actually
/// holds — "what lights are on?", "any lights offline?", "check the lights"
/// (ADR 0076). Four gates, all required:
///
/// 1. **A kind the house knows.** The kind vocabulary is the first word of the
///    service's own keys (`light.kitchen_table`), plural-trimmed — never an
///    English list, the same rule `note_not_answering` follows.
/// 2. **A question-or-status word.** Without one, "I love these lights" is
///    conversation.
/// 3. **No action verb.** "Turn off the lights" carries both `off` and `lights`
///    and is an instruction: it falls through to the model's turn, where policy
///    gates the acting. This gate is what makes the other two safe.
/// 4. **About now, and about this house.** A time such as "this month" or
///    "yesterday", or "in <a place the house has no word for>", means the person is
///    not asking for the house's current reading. Everyday words (`event`,
///    `weather`, `calendar`) are kinds too, so without this a question about a city
///    was answered with a count of the house's entities.
pub(super) fn asked_about_a_kind(text: &str, states: &[(String, String)]) -> Option<String> {
    let words = words_of(text);
    const ACTS: &[&str] = &[
        "turn", "switch", "set", "dim", "brighten", "make", "put", "start", "stop", "play",
        "pause", "open", "close", "lock", "unlock",
    ];
    if words.iter().any(|w| ACTS.contains(&w.as_str())) {
        return None;
    }
    const ASKS: &[&str] = &[
        "on",
        "off",
        "offline",
        "unavailable",
        "status",
        "check",
        "working",
        "ok",
        "okay",
        "running",
        "many",
        "any",
        "which",
        "what",
    ];
    if !words.iter().any(|w| ASKS.contains(&w.as_str())) {
        return None;
    }
    // The reading is of NOW, in THIS house, so a question about another time or another
    // place is not asking for it. Measured live: "what watch events are in new york this
    // month?" matched a Home Assistant `event` entity and was answered with the house's
    // count of them. Everyday nouns (`event`, `weather`, `calendar`) are kinds here too.
    // Handing a question on costs a model turn; answering the wrong one costs trust.
    const OTHER_TIMES: &[&str] = &[
        "tomorrow",
        "yesterday",
        "tonight",
        "week",
        "weekend",
        "month",
        "year",
        "upcoming",
        "scheduled",
        "next",
        "last",
    ];
    if words.iter().any(|w| OTHER_TIMES.contains(&w.as_str())) {
        return None;
    }
    let kinds: std::collections::HashSet<String> = states
        .iter()
        .filter_map(|(id, _)| words_of(id).into_iter().next())
        .collect();
    // "in <place>": fine when the house knows the place by name (a word in one of its own
    // keys — `light.guest_bedroom_left` knows "guest" and "bedroom") or when it is just the
    // house; otherwise the person is asking about somewhere else.
    const THE_HOUSE: &[&str] = &[
        "house",
        "home",
        "place",
        "apartment",
        "flat",
        "building",
        "room",
        "downstairs",
        "upstairs",
        "everywhere",
        "here",
    ];
    const DETERMINERS: &[&str] = &["the", "my", "our", "this", "that", "a", "an", "any"];
    let house_words: std::collections::HashSet<String> =
        states.iter().flat_map(|(id, _)| words_of(id)).collect();
    let known = |w: &str| {
        let singular = w.strip_suffix('s').unwrap_or(w);
        house_words.contains(w)
            || house_words.contains(singular)
            || THE_HOUSE.contains(&w)
            || THE_HOUSE.contains(&singular)
    };
    for (at, word) in words.iter().enumerate() {
        if word != "in" {
            continue;
        }
        let place = words[at + 1..]
            .iter()
            .find(|next| !DETERMINERS.contains(&next.as_str()));
        if place.is_some_and(|place| !known(place)) {
            return None;
        }
    }
    words
        .iter()
        .map(|w| w.strip_suffix('s').unwrap_or(w).to_owned())
        .find(|w| kinds.contains(w))
}

/// The house's answer about one kind of thing, from the reading itself (ADR 0076).
///
/// Deterministic and complete where completeness is the point: what is **on** and
/// what is **not answering** are named (those are the two lists a person acts on),
/// everything else is counted. `unknown` is not a problem (ADR 0056's vocabulary),
/// and a kind the house holds nothing of answers `None` so the ordinary turn can
/// have the conversation instead.
pub(super) fn kind_status_answer(kind: &str, states: &[(String, String)]) -> Option<String> {
    const NAMED: usize = 8;
    let pretty = |id: &str| {
        id.split_once('.')
            .map_or(id, |(_, rest)| rest)
            .replace('_', " ")
    };
    let of_kind: Vec<(&String, &String)> = states
        .iter()
        .filter(|(id, _)| words_of(id).first().map(String::as_str) == Some(kind))
        .map(|(id, state)| (id, state))
        .collect();
    if of_kind.is_empty() {
        return None;
    }
    let named = |list: &[String]| {
        let shown: Vec<String> = list.iter().take(NAMED).cloned().collect();
        let more = list.len().saturating_sub(NAMED);
        if more > 0 {
            format!("{} and {more} more", shown.join(", "))
        } else {
            shown.join(", ")
        }
    };
    let on: Vec<String> = of_kind
        .iter()
        .filter(|(_, s)| s.eq_ignore_ascii_case("on"))
        .map(|(id, _)| pretty(id))
        .collect();
    let dead: Vec<String> = of_kind
        .iter()
        .filter(|(_, s)| endora_capabilities::not_answering(s))
        .map(|(id, _)| pretty(id))
        .collect();
    let off = of_kind
        .iter()
        .filter(|(_, s)| s.eq_ignore_ascii_case("off"))
        .count();
    let mut parts: Vec<String> = Vec::new();
    if !on.is_empty() {
        parts.push(format!("{} on — {}", on.len(), named(&on)));
    }
    if off > 0 {
        parts.push(format!("{off} off"));
    }
    if !dead.is_empty() {
        parts.push(format!("{} not answering — {}", dead.len(), named(&dead)));
    }
    let accounted = on.len() + off + dead.len();
    let rest = of_kind.len() - accounted;
    if rest > 0 {
        parts.push(format!("{rest} reporting other readings"));
    }
    Some(format!(
        "Of your {} {kind}{}: {}.",
        of_kind.len(),
        if of_kind.len() == 1 { "" } else { "s" },
        parts.join(" · ")
    ))
}

/// The brief's wording pass, shared by the scheduled brief and a chat ask: the deep
/// model writes it when the person opted in (append-never-rewrite, ADR 0056), and
/// the facts stand as themselves otherwise.
pub(super) fn worded_brief(
    butler: &dyn Butler,
    facts: &str,
    ids: &impl IdSource,
    clock: &impl Clock,
) -> String {
    match butler
        .deeper()
        .and_then(|deeper| word_the_brief(&deeper, facts, ids, clock))
    {
        // Cut off at the token ceiling. What arrived is real but partial, and the
        // facts it never reached are not facts it *left out* — so asking what it
        // omitted (append-never-rewrite, ADR 0056) answers a question that only
        // makes sense about a reply that finished. Measured live: the appended
        // answer was the whole raw gather — `IMAP (…) Messages: 38303`, the raw
        // weather and headlines — printed under a sentence that stopped mid-word.
        //
        // So the facts post as themselves, which ADR 0074 already calls a brief:
        // complete, plainly worded, and nothing lost. Half a brief with the raw
        // material beneath it is the one shape that is worse than either.
        Some(written) if written.cut_short => facts.to_owned(),
        Some(written) => match not_yet_said(&written.text, facts) {
            missed if missed.is_empty() => written.text,
            missed => format!("{}\n\n{}", written.text, missed.join("\n")),
        },
        None => facts.to_owned(),
    }
}

/// Composes a **daily briefing** — an act of service (ADR 0056), assembled as a
/// standing order (ADR 0074). The sections are the person's own: their calendar and
/// presence, what has stopped answering, their weather (with whether an umbrella or
/// jacket is worth it), their drive, their top three headlines, their local events —
/// each gathered by code from a configured skill with known-good arguments. The
/// model's only job is the wording, and only the deep model gets it (measured: the
/// local one, handed exactly these facts, wrote about lights instead).
///
/// What ADR 0074 retires is the agentic gather: a brief that depended on the model
/// choosing the right tools arrived as "the kitchen and garage lights are on" four
/// days running, and the person's own request — three headlines, the weather, the
/// drive — had nowhere to live but a prompt the model was free to ignore. The facts
/// no longer need a model at all: with no deep wording they post as themselves.
/// Append, never rewrite (ADR 0056) still governs the worded path.
///
/// `None` — no brief — only when there was genuinely nothing to say.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
#[allow(clippy::too_many_arguments)]
pub fn daily_brief(
    chat: &impl ChatRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    // Lines added after the brief is worded, verbatim: what must never reach the model that
    // words it, which may be the paid one (the mail section, ADR 0097).
    appended: &[String],
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let mut activity: Vec<String> = Vec::new();
    let brief_ctx = ButlerContext {
        now: clock_line(clock),
        ..context.clone()
    };
    let Some(facts) = assembled_brief_facts(capabilities, &brief_ctx, &mut activity) else {
        // Nothing known and nothing gathered: no brief. A scripted one would be
        // Endora claiming to have thought about the day (ADR 0053).
        return Ok(None);
    };
    // The brief is worded by the deep model when the person has opted in (ADR 0055).
    //
    // Measured, not assumed: given exactly these facts the local model wrote "the
    // lights in your bedroom are off", and the deep one wrote a brief that used every
    // one of them — so the local model is not asked at all, and without a deep model
    // the facts stand as their own brief.
    //
    // **Gathering stays local.** Only the assembled facts leave, disguised — and only
    // those: the deep model gets a context with nothing else in it, so beliefs,
    // aliases and the track record never travel just because prose is wanted.
    //
    // Append, never rewrite (ADR 0056): whatever the writing left out is appended
    // verbatim. Appending everything printed the brief twice on its first live run;
    // appending nothing would trust the model, which is what the appending exists to
    // avoid.
    let mut text = worded_brief(butler, &facts, ids, clock);
    if !appended.is_empty() {
        text.push_str("\n\n");
        text.push_str(&appended.join("\n"));
    }
    let message = post_butler_message(chat, ids, clock, &text)?;
    Ok(Some((message, activity)))
}

/// Posts a butler message to the chat (used by out-of-band paths like the deep-model
/// answer). Returns the persisted message.
///
/// # Errors
/// [`AppError::Domain`] if the text is blank, or [`AppError::Repository`] on failure.
pub fn post_butler_message(
    chat: &impl ChatRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    text: &str,
) -> Result<ChatMessage, AppError> {
    let message = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        text,
        clock.now(),
    )?;
    chat.append(&message)?;
    Ok(message)
}

/// Posts a person's message to the chat (used by out-of-band paths like a manual
/// "Ask deep", so the exchange shows both sides). Returns the persisted message.
///
/// # Errors
/// [`AppError::Domain`] if the text is blank, or [`AppError::Repository`] on failure.
pub fn post_user_message(
    chat: &impl ChatRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    text: &str,
) -> Result<ChatMessage, AppError> {
    let message = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        text,
        clock.now(),
    )?;
    chat.append(&message)?;
    Ok(message)
}

/// Returns the daily-brief schedule, defaulting to **off**.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn brief_schedule(briefs: &impl BriefScheduleRepository) -> Result<BriefSchedule, AppError> {
    Ok(briefs
        .get()?
        .unwrap_or_else(BriefSchedule::disabled_default))
}

/// Turns the daily brief on/off and sets the UTC hour it prepares at.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn set_brief_schedule(
    briefs: &impl BriefScheduleRepository,
    enabled: bool,
    hour_utc: u8,
) -> Result<BriefSchedule, AppError> {
    let current = briefs
        .get()?
        .unwrap_or_else(BriefSchedule::disabled_default);
    let schedule = BriefSchedule {
        enabled,
        hour_utc: hour_utc.min(23),
        // Keep last_at so toggling doesn't re-fire the same day.
        last_at: current.last_at,
    };
    briefs.set(&schedule)?;
    Ok(schedule)
}

/// If a daily brief is **due** (enabled, the hour matches, none prepared today),
/// prepares one via [`daily_brief`] and records that it fired. Called by the
/// heartbeat. Returns the posted brief + activity, or `None`.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn run_due_brief(
    chat: &impl ChatRepository,
    briefs: &impl BriefScheduleRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    appended: &[String],
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let now = clock.now();
    let Some(mut schedule) = briefs.get()? else {
        return Ok(None);
    };
    if !schedule.is_due(now) {
        return Ok(None);
    }
    // A scheduled brief is unsolicited. If the person spoke recently, they are already
    // present and the brief would be easy to mistake for the answer to their question
    // (the phone screenshot that prompted this guard showed exactly that failure).
    // Leave the schedule untouched so the next daily window can try again.
    if person_spoke_recently(chat, now)? {
        return Ok(None);
    }
    // Mark fired first, so a slow compose can't double-post on the next tick.
    schedule.last_at = now;
    briefs.set(&schedule)?;
    daily_brief(chat, capabilities, butler, ids, clock, context, appended)
}

/// Whether the person has spoken within the quiet period used by unsolicited work.
/// Reads only the quiet window, never the whole history: this runs on every heartbeat
/// while a brief is due and the person is chatting.
pub(super) fn person_spoke_recently(
    chat: &impl ChatRepository,
    now: Timestamp,
) -> Result<bool, AppError> {
    const QUIET_AFTER_ACTIVITY_MS: i64 = 60 * 60 * 1_000;
    let from = now
        .unix_millis()
        .saturating_sub(QUIET_AFTER_ACTIVITY_MS - 1);
    Ok(chat
        .between(from, now.unix_millis().saturating_add(1))?
        .iter()
        .any(|message| message.role() == MessageRole::User))
}
