use super::*;

/// Persists the beliefs the butler formed — Endora's own understanding (not
/// actions), kept directly then reviewable/correctable (ADR 0052). If the butler
/// restated something Endora already believes, the existing belief is **affirmed**
/// (raising confidence) rather than storing a near-duplicate. Each stored/affirmed
/// belief is appended to `activity` in plain language. Shared by the chat turn and
/// the nightly self-improvement loop (ADR 0051) — the "reflect" step.
///
/// # Errors
/// [`AppError::Domain`] if a statement is invalid, or [`AppError::Repository`] on
/// a backend failure.
pub(super) fn record_formed_beliefs(
    beliefs: &impl BeliefRepository,
    formed: Vec<FormedBelief>,
    ids: &impl IdSource,
    clock: &impl Clock,
    activity: &mut Vec<String>,
) -> Result<(), AppError> {
    // Grows as the batch stores. Read once and left alone, a turn that forms two
    // paraphrases of one thought stores both — which is exactly how three cards saying
    // "Fahrenheit" ended up on one screen, each asking to be confirmed on its own.
    let mut existing = beliefs.list()?;
    for belief in formed {
        // A command the person gave is not a fact about them. Dropping these keeps
        // the model of the person from filling up with spent instructions.
        if reads_as_an_instruction(&belief.statement) {
            continue;
        }
        // Nor is Endora's own conduct evidence about the person (ADR 0052).
        if is_about_endora_itself(&belief.evidence) {
            continue;
        }
        // Nor is an event: tied to "tomorrow", it is wrong the next day and outranks the
        // calendar that has it right.
        if pinned_to_a_day(&belief.statement) {
            continue;
        }
        // Only durable kinds are understanding (owner, 2026-10-16). An "intent" was a one-off
        // question filed as a goal of theirs, and "other" a passing remark or an event.
        if matches!(
            belief.kind,
            crate::BeliefKind::Intent | crate::BeliefKind::Other
        ) {
            continue;
        }
        if let Some(mut prior) = existing
            .iter()
            .find(|b| similar(b.statement(), &belief.statement))
            .cloned()
        {
            activity.push(format!("Grew more sure that {}", prior.statement()));
            prior.affirm(clock.now());
            beliefs.save(&prior)?;
            if let Some(slot) = existing.iter_mut().find(|b| b.id() == prior.id()) {
                *slot = prior;
            }
            continue;
        }
        // Surface a disagreement rather than resolving it. Endora holding two
        // contradictory beliefs means it is wrong about something, which is the most
        // useful thing understanding can tell the person — and which of them is true
        // is exactly the judgement that belongs to them, not to the butler
        // (constitution §4: distinguish evidence from assumption, never present a
        // guess as a fact). Both are kept; the person resolves it by correcting one.
        for conflicting in existing
            .iter()
            .filter(|b| b.status() == crate::BeliefStatus::Active)
            .filter(|b| statements_disagree(b.statement(), &belief.statement))
        {
            activity.push(format!(
                "Noticed this sits oddly with something I already thought: \"{}\" vs \"{}\"",
                conflicting.statement(),
                belief.statement.trim()
            ));
        }
        activity.push(format!("Learned that {}", belief.statement.trim()));
        let stored = Belief::new(
            BeliefId::new(ids.new_id()),
            &belief.statement,
            belief.kind,
            belief.confidence,
            &belief.evidence,
            clock.now(),
        )?;
        beliefs.save(&stored)?;
        existing.push(stored);
    }
    Ok(())
}

/// Re-reads what is already stored against the rules as they stand today (ADR 0052).
///
/// Every guard on understanding ran **at formation only**, which quietly means the model
/// of the person is frozen at the quality of the rules on the day each belief was formed.
/// Observed: a card reading *you want me to turn off the kitchen light — because turn off
/// the kitchen light entity*, sitting there long after the rule that rejects exactly that
/// shape shipped. It was never going to leave, because nothing ever looked at it again.
///
/// Two rules, both already written and both applied here to the past rather than the
/// future:
///
/// - a statement that reads as an **instruction** is a spent command, not a fact about a
///   person, and is expired;
/// - a statement that says what an **older** one already says is expired, and the older one
///   is affirmed — the same thought arriving twice is genuinely evidence for it.
///
/// [`BeliefStatus::Expired`] rather than deletion or `Corrected`: the person did not say it
/// was wrong, so claiming they did would put words in their mouth, and a belief Endora
/// dropped on its own is worth being able to see.
///
/// Returns how many it retired, for the activity trail.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
pub fn tidy_understanding(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<usize, AppError> {
    let mut held: Vec<Belief> = beliefs
        .list()?
        .into_iter()
        .filter(|b| b.status() == crate::BeliefStatus::Active)
        .collect();
    // Oldest first, so the belief that survives a merge is the one that has been held
    // longest — it carries the earlier evidence and the longer history of affirmation.
    held.sort_by_key(|b| b.created_at().unix_millis());
    let mut kept: Vec<Belief> = Vec::new();
    let mut retired = 0;
    for mut belief in held {
        if reads_as_an_instruction(belief.statement()) || is_about_endora_itself(belief.evidence())
        {
            belief.expire();
            beliefs.save(&belief)?;
            retired += 1;
            continue;
        }
        if let Some(older) = kept
            .iter_mut()
            .find(|k| similar(k.statement(), belief.statement()))
        {
            older.affirm(clock.now());
            beliefs.save(older)?;
            belief.expire();
            beliefs.save(&belief)?;
            retired += 1;
            continue;
        }
        kept.push(belief);
    }
    Ok(retired)
}

/// Verbs that name a change to the world rather than something about the person.
pub(super) const ACTION_VERBS: &[&str] = &[
    "turn", "switch", "set", "play", "pause", "resume", "dim", "brighten", "lock", "unlock",
    "open", "close", "start", "stop", "send", "add", "remove", "delete", "book", "order", "buy",
    "schedule", "cancel", "call", "text", "email", "post", "put", "move",
];

/// Whether a proposed belief is tied to a day ("tomorrow", "today", "tonight",
/// "yesterday"). That is an event, not understanding: it is wrong the next day, and the model
/// trusts a belief over the calendar that has the event right.
pub fn pinned_to_a_day(statement: &str) -> bool {
    let lowered = statement.to_lowercase();
    lowered
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| matches!(w, "today" | "tomorrow" | "tonight" | "yesterday"))
}

/// Whether a "belief" is really a **one-off instruction** the person gave, rather
/// than something true about them.
///
/// Observed live: the butler filed "you want me to turn off the kitchen light" as a
/// durable preference, and later "you want me to turn on the kitchen lights" beside
/// it. Neither is understanding — they are two commands from two moments, recorded
/// as if they described a person. Left alone they accumulate, contradict each other,
/// and pollute the context every later turn reasons from.
///
/// The discriminator is **what follows the request**. "You want me to *be more
/// direct*" is a genuine standing preference about how Endora should behave and must
/// be kept; "you want me to *turn off* the light" names an action on the world and is
/// spent the moment it is carried out.
pub fn reads_as_an_instruction(statement: &str) -> bool {
    let text = normalized(statement);
    let addressed_to_endora = [
        "want me to",
        "asked me to",
        "told me to",
        "need me to",
        "would like me to",
        "d like me to",
        "wanted me to",
    ]
    .iter()
    .any(|phrase| text.contains(phrase));
    if !addressed_to_endora {
        return false;
    }
    polarity_tokens(&text)
        .iter()
        .any(|w| ACTION_VERBS.contains(&w.as_str()))
}

/// Whether the evidence for a belief is Endora describing **itself**.
///
/// Live, top of the understanding screen:
///
/// ```text
/// you find it more convenient for the assistant to wait for instructions
///   because I didn't do anything proactive since I rely on your instructions.
/// ```
///
/// Endora had been passive — because its direct reach into the house was broken — and
/// concluded from its own conduct that the person prefers passivity. It then carried that
/// into every later turn as something it knew about them.
///
/// This is the sharpest possible violation of the rule that understanding admits **only
/// facts about the person** (ADR 0052): a butler that mistakes its own behaviour for
/// evidence will reinforce whatever it happens to be doing, including its own faults.
///
/// The discriminator is narrow — evidence whose **first word is "I"** is Endora as the
/// subject. *"You asked 'where did you find this?' after I listed some events"* keeps its
/// belief, because the person is the subject and Endora only appears in passing.
pub(super) fn is_about_endora_itself(evidence: &str) -> bool {
    let mut words = evidence.split_whitespace();
    let first = words
        .next()
        .unwrap_or_default()
        .trim_matches(|c: char| !c.is_alphanumeric());
    first.eq_ignore_ascii_case("i")
}

/// Normalizes a belief statement for duplicate detection: lowercase, collapse
/// whitespace, drop trailing punctuation.
pub(super) fn normalized(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .to_owned()
}

/// Reduces a word to a crude stem so inflections match: `retirement`/`retire`,
/// `travelling`/`travel`, `flights`/`flight`. Deliberately simple — this only has
/// to make duplicate detection robust, not to be a linguistically correct stemmer,
/// and an over-eager one would merge genuinely different beliefs.
pub(super) fn stem(word: &str) -> String {
    for suffix in [
        "ements", "ement", "ations", "ation", "ings", "ing", "ies", "ed", "es", "s",
    ] {
        if let Some(root) = word.strip_suffix(suffix) {
            // Keep a recognisable root; "ties" must not collapse to "t".
            if root.len() >= 3 {
                let root = if suffix == "ies" {
                    format!("{root}y")
                } else {
                    root.to_owned()
                };
                return trim_stem(&root);
            }
        }
    }
    trim_stem(word)
}

/// Drops a trailing `e` and un-doubles a final consonant, so British and American
/// inflections land on one stem (`travelling` → `travell` → `travel`).
pub(super) fn trim_stem(root: &str) -> String {
    let root = root.trim_end_matches('e');
    let mut chars: Vec<char> = root.chars().collect();
    if chars.len() > 3 {
        let last = chars[chars.len() - 1];
        if last == chars[chars.len() - 2] && !"aeiou".contains(last) {
            chars.pop();
        }
    }
    chars.into_iter().collect()
}

/// Content words of a statement, stemmed — drops filler so two phrasings of the
/// same belief share keywords. The stop list covers the scaffolding a butler
/// naturally varies between turns ("you want X" / "you'd like to have X").
pub(super) fn keywords(s: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "you", "your", "are", "the", "and", "for", "that", "this", "with", "have", "was", "its",
        "but", "not", "can", "want", "like", "get", "getting", "able", "being", "been", "once",
        "when", "while", "into", "about", "some", "more", "much", "very", "really", "just",
        "still", "also", "would", "could", "should", "them", "they", "their", "she", "him", "her",
        "his", "who", "why", "how", "what", "where", "than", "then", "there", "here",
    ];
    normalized(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !STOP.contains(w))
        .map(stem)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Every word of a statement, lowercased with apostrophes closed up so `don't`
/// becomes `dont`. Unlike [`keywords`] this drops **nothing** — the words that flip
/// a statement's meaning (`on`, `no`, `not`) are exactly the short, common ones a
/// keyword filter throws away.
pub(super) fn polarity_tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace(['\'', '\u{2019}'], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Words that reverse a statement's sense. Asymmetry in these means two statements
/// disagree however much vocabulary they share.
pub(super) const NEGATIONS: &[&str] = &[
    "not", "never", "no", "dont", "doesnt", "didnt", "wont", "isnt", "arent", "cant", "cannot",
    "without", "instead", "rather", "dislike", "dislikes", "hate", "hates", "avoid", "avoids",
];

/// Pairs where holding one side contradicts the other.
pub(super) const ANTONYMS: &[(&str, &str)] = &[
    ("on", "off"),
    ("more", "less"),
    ("always", "never"),
    ("like", "dislike"),
    ("prefer", "avoid"),
    ("before", "after"),
    ("morning", "evening"),
    ("morning", "night"),
    ("early", "late"),
    ("hot", "cold"),
    ("warm", "cool"),
    ("start", "stop"),
    ("open", "close"),
    ("lock", "unlock"),
    ("increase", "decrease"),
    ("up", "down"),
    ("fahrenheit", "celsius"),
    ("metric", "imperial"),
];

/// Whether two statements **disagree** — one negates where the other does not, or
/// they sit on opposite sides of an antonym pair.
///
/// This is checked on the raw words rather than the stemmed keywords, because the
/// decisive tokens are the ones keyword filtering removes: `on` is two characters,
/// `like` is a stopword, and `don't` splits into fragments. Without this,
/// "turn **off** the kitchen light" and "turn **on** the kitchen lights" reduce to
/// the same keyword set and merge into one belief — silently discarding the
/// disagreement instead of surfacing it.
#[must_use]
pub fn statements_disagree(a: &str, b: &str) -> bool {
    // Two statements can only contradict each other if they are **about** the same thing.
    // Without this, negation asymmetry alone counts: "you find it more convenient to
    // measure temperature in Celsius rather than Fahrenheit" was read as disagreeing with
    // every other belief Endora held, including "you want to know where I found the event
    // information". Harmless while the answer only fed duplicate detection, which also
    // required overlap; put on screen, it accused every card of contradicting one belief
    // about thermometers.
    if !about_the_same_thing(a, b) {
        return false;
    }
    let (ta, tb) = (polarity_tokens(a), polarity_tokens(b));
    let negated = |t: &[String]| t.iter().any(|w| NEGATIONS.contains(&w.as_str()));
    if negated(&ta) != negated(&tb) {
        return true;
    }
    let has = |t: &[String], w: &str| t.iter().any(|x| x == w);
    ANTONYMS
        .iter()
        .any(|(x, y)| (has(&ta, x) && has(&tb, y)) || (has(&ta, y) && has(&tb, x)))
}

/// Whether two belief statements are effectively the same (a paraphrase), by
/// stemmed keyword overlap — public so the live smoke check can assert the same rule the
/// tidy pass applies, rather than a second implementation of it that can drift — so "you want more energy so you can travel when you
/// retire" and "you're motivated by being able to travel in retirement" are one
/// belief, not two.
///
/// Uses **containment** rather than symmetric Jaccard: if one statement's keywords
/// are largely a subset of the other's, it says nothing new. A plain Jaccard
/// penalises the longer, more specific phrasing and lets near-duplicates through —
/// which is what let them accumulate before (the `no-duplicate` eval case, ADR 0055).
/// The threshold stays deliberately high: **wrongly merging two distinct beliefs
/// silently loses understanding, which is worse than storing one duplicate the
/// person can correct.**
///
/// Statements that [disagree](statements_disagree) are **never** similar, however
/// much wording they share. Two contradictory beliefs are the most informative thing
/// understanding can contain — they mean Endora is wrong about something — and
/// collapsing them into one keeps whichever arrived first and destroys the signal.
#[must_use]
pub fn says_the_same_thing(a: &str, b: &str) -> bool {
    similar(a, b)
}

/// See [`says_the_same_thing`] for the public form.
pub(super) fn similar(a: &str, b: &str) -> bool {
    if statements_disagree(a, b) {
        return false;
    }
    let (ka, kb) = (subject_words(a), subject_words(b));
    if ka.is_empty() || kb.is_empty() {
        return normalized(a) == normalized(b);
    }
    let shared = ka.iter().filter(|w| kb.contains(w)).count() as f64;
    // **Both** statements, not the shorter one.
    //
    // Dividing by the smaller set is containment, and containment says a short belief is a
    // copy of any longer one that happens to mention the same things. Live, it called these
    // the same thought:
    //
    //     "you prefer to run after work on Tuesdays and Thursdays"
    //     "Work unpredictability squeezes your evenings — a 4-hour issue call pushed your
    //      main work late — so the after-work run needs to stay small enough to survive a
    //      late day."
    //
    // They share a subject and say entirely different things. One is a preference, the
    // other is what the week does to it. Under containment the specific one swallows the
    // general one — and because this same rule decides whether a night's thought is a
    // discovery, **a detailed belief quietly stops related simpler ones from ever forming**.
    // That is the opposite of learning, in the function that decides what is learned.
    //
    // Jaccard is symmetric: sameness has to hold in both directions, so a longer statement
    // must actually be about the same amount of thing to count as a repeat.
    let union = ka.len() + kb.len() - shared as usize;
    if union == 0 {
        return normalized(a) == normalized(b);
    }
    shared / union as f64 >= 0.6
}

/// Whether two statements concern the same subject at all — the precondition for them
/// being able to agree or disagree.
///
/// Half of the shorter statement's subject words, which is a much weaker bar than
/// [`similar`]'s: *you like running* and *you hate running* are about the same thing
/// without being the same belief, and that gap is precisely where contradiction lives.
pub(super) fn about_the_same_thing(a: &str, b: &str) -> bool {
    let (sa, sb) = (subject_words(a), subject_words(b));
    if sa.is_empty() || sb.is_empty() {
        return false;
    }
    let shared = sa.iter().filter(|w| sb.contains(w)).count() as f64;
    shared / sa.len().min(sb.len()) as f64 >= 0.5
}

/// Words that describe **having a stance** rather than what the stance is about.
///
/// Three cards said the same thing on one screen — *prefer temperatures in Fahrenheit*,
/// *prefer temperature measurements in Fahrenheit*, *find it more convenient and accurate
/// to measure temperature in Fahrenheit*. Only the first two matched, because the third
/// spends most of its words on how strongly the person feels and only two on the subject.
/// Comparing everything means a wordier paraphrase looks like a different belief.
///
/// Polarity words are stripped **too** — `like` alongside `hate` — which is deliberate and
/// only safe because of the division of labour above: [`subject_words`] decides whether two
/// statements are about the same thing, and [`statements_disagree`] (checked first) decides
/// whether they say opposite things about it. Keeping only the positive ones here would
/// merge *you like running* with *you hate running* whenever disagreement missed it.
pub(super) const STANCE_WORDS: &[&str] = &[
    "prefer",
    "want",
    "like",
    "love",
    "enjoy",
    "hate",
    "dislik",
    "avoid",
    "find",
    "feel",
    "think",
    "believ",
    "convenient",
    "accurat",
    "comfort",
    "usual",
    "typical",
    "general",
    "realli",
    "more",
    "most",
    "veri",
    "would",
    "rather",
];

/// What a statement is **about**, with the stance words removed.
///
/// Falls back to the full keywords when nothing survives: *you'd really rather not* is all
/// stance and no subject, and comparing two empty sets would merge every such statement
/// with every other.
pub(super) fn subject_words(s: &str) -> Vec<String> {
    let all = keywords(s);
    let subject: Vec<String> = all
        .iter()
        .filter(|w| !STANCE_WORDS.contains(&w.as_str()))
        .cloned()
        .collect();
    if subject.is_empty() { all } else { subject }
}

/// A short, human present-tense label for a skill in progress — what the butler
/// shows while it works a step ("· Checking the weather…"), so the person can see
/// it moving toward the goal rather than sitting on a silent "one moment".
pub(super) fn progress_label(id: &str) -> String {
    match id {
        "weather" => "Checking the weather",
        "news" | "local_news" => "Checking the news",
        "sports_schedule" => "Checking the team's schedule",
        "safety_alerts" => "Checking safety alerts",
        "web_search" | "web_answers" => "Searching the web",
        "web_fetch" => "Reading the page",
        "knowledge" => "Looking that up",
        "home_assistant" => "Checking your home",
        "city_meetings" => "Checking the city's agenda",
        "image_review" => "Looking at the image",
        other => return format!("Using the {other} skill"),
    }
    .to_owned()
}

/// The current date and time where the person is, for the model: "Friday, 2026-10-16 20:30
/// (UTC-04:00)", or the UTC form when the clock does not know the zone. It was always UTC, so
/// on a summer evening the model was told it was already the next day.
pub(crate) fn clock_line(clock: &(impl Clock + ?Sized)) -> String {
    let now = clock.now().unix_millis();
    let offset = clock.utc_offset_minutes();
    if offset == 0 {
        return format_datetime_utc(now);
    }
    let local = format_datetime_utc(now + offset * 60_000);
    let wall = local.trim_end_matches(" UTC");
    let sign = if offset < 0 { '-' } else { '+' };
    let (h, m) = (offset.abs() / 60, offset.abs() % 60);
    format!("{wall} (UTC{sign}{h:02}:{m:02})")
}

/// Formats a Unix-millisecond timestamp as `"Weekday, YYYY-MM-DD HH:MM UTC"` — no
/// date dependency, using the standard civil-from-days algorithm. For the person's own
/// time, [`clock_line`].
pub(crate) fn format_datetime_utc(ms: i64) -> String {
    let day = ms.div_euclid(86_400_000);
    let secs = ms.rem_euclid(86_400_000) / 1000;
    let (hh, mm) = (secs / 3600, (secs % 3600) / 60);
    // Weekday: Unix day 0 (1970-01-01) was a Thursday.
    const DOW: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let dow = DOW[(day.rem_euclid(7) + 4).rem_euclid(7) as usize];
    // Civil date from days since epoch (Howard Hinnant's algorithm).
    let z = day + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    format!("{dow}, {year:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
}

/// Endora's living understanding of the person — its active beliefs, most
/// recently affirmed first. Corrected beliefs are omitted.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn understanding(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Vec<Belief>, AppError> {
    let now = clock.now();
    Ok(beliefs
        .list()?
        .into_iter()
        .filter(|b| b.status() == crate::BeliefStatus::Active)
        // Beliefs weaken without reinforcement and eventually fade out entirely
        // (ADR 0052). Filtering on read means understanding is honest the moment it
        // is asked for, whether or not the nightly loop has run.
        .filter(|b| !b.has_faded(now))
        .collect())
}

/// Ages understanding: marks every faded belief as expired, so Endora stops acting
/// on things nothing has supported in a long time and the person can see that it
/// let them go. Run by the nightly loop.
///
/// Read-side filtering already hides faded beliefs, so this changes no behaviour —
/// it makes the forgetting **durable and visible** rather than implicit, which is
/// what the memory rights require of anything Endora holds (constitution §6).
/// Returns the statements it expired, for the activity trail.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn expire_faded_beliefs(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Vec<String>, AppError> {
    let now = clock.now();
    let mut expired = Vec::new();
    for mut belief in beliefs.list()? {
        if !belief.has_faded(now) {
            continue;
        }
        expired.push(belief.statement().to_owned());
        belief.expire();
        beliefs.save(&belief)?;
    }
    Ok(expired)
}

/// The person confirms a belief is right: raise its confidence.
///
/// # Errors
/// [`AppError::NotFound`] if it does not exist, or [`AppError::Repository`].
pub fn affirm_belief(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
    id: BeliefId,
) -> Result<Belief, AppError> {
    let mut belief = beliefs
        .get(id)?
        .ok_or(AppError::NotFound { entity: "belief" })?;
    belief.affirm(clock.now());
    beliefs.save(&belief)?;
    Ok(belief)
}

/// How many recent outcomes the track record is built from. Bounded because the
/// prompt has to stay small on a slow local model, and because how an action landed
/// last month says little about now.
pub(super) const TRACK_RECORD_WINDOW: usize = 50;

/// What Endora is pursuing and has pursued, most recently moved first (ADR 0052).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn intentions(intentions: &impl IntentionRepository) -> Result<Vec<Intention>, AppError> {
    Ok(intentions.list()?)
}

/// What Endora has noticed is wrong with its own tooling (ADR 0054).
///
/// Derived from outcome history on every read — there is no store of proposals, nothing
/// to dismiss and nothing to groom, which is how ADR 0052's approval queue is made
/// impossible rather than merely discouraged. A proposal disappears on its own when the
/// outcomes age out or an action finally changes something.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn repairs(
    outcomes: &impl OutcomeRepository,
    answered: &[(String, String)],
) -> Result<Vec<RepairProposal>, AppError> {
    Ok(endora_understanding::repair_proposals(
        &outcomes.list()?,
        answered,
    ))
}

/// The person tells Endora to stop working on something (ADR 0052).
///
/// Their whole authority over an intention, and deliberately the only verb they have:
/// Endora forms its own, from what it understands, and there is no path by which the
/// person can create or edit one.
///
/// # Errors
/// [`AppError::NotFound`] if there is no such intention, or [`AppError::Repository`].
pub fn drop_intention(
    intentions: &impl IntentionRepository,
    id: IntentionId,
) -> Result<Intention, AppError> {
    let mut intention = intentions.get(id)?.ok_or(AppError::NotFound {
        entity: "intention",
    })?;
    intention.abandon();
    intentions.save(&intention)?;
    Ok(intention)
}

/// The outcomes of what Endora has done, most recent first (ADR 0053) — the memory
/// right to *see* what it did, alongside the beliefs it holds.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn recent_outcomes(
    outcomes: &impl OutcomeRepository,
    limit: usize,
) -> Result<Vec<Outcome>, AppError> {
    Ok(outcomes.list()?.into_iter().take(limit).collect())
}

/// The person says how an action landed (ADR 0053). They are never asked for this;
/// it is offered where the action already appears, and the latest word wins.
///
/// # Errors
/// [`AppError::NotFound`] if there is no such outcome, or [`AppError::Repository`].
pub fn react_to_outcome(
    outcomes: &impl OutcomeRepository,
    id: OutcomeId,
    reaction: Reaction,
) -> Result<Outcome, AppError> {
    let mut outcome = outcomes
        .get(id)?
        .ok_or(AppError::NotFound { entity: "outcome" })?;
    outcome.react(reaction);
    outcomes.save(&outcome)?;
    Ok(outcome)
}

/// How the butler's past actions have landed, per skill — one line each (ADR 0053).
///
/// **Only skills the person has actually reacted to appear.** An action nobody
/// commented on says nothing about whether it helped, and padding the prompt with
/// "5 uses, no feedback" would spend a slow local model's context on noise. Empty
/// until they have said something, which is the normal early state.
///
/// Ordered by how much was said about a skill, so the most-judged comes first, and
/// capped — this is a nudge, not a report.
pub(super) fn track_record(outcomes: &[Outcome]) -> Vec<String> {
    /// Skills listed in the prompt at most.
    const MAX_SKILLS: usize = 5;
    let mut by_skill: std::collections::BTreeMap<&str, (usize, usize)> =
        std::collections::BTreeMap::new();
    for outcome in outcomes {
        let entry = match outcome.reaction() {
            Some(Reaction::Helped) => (1, 0),
            Some(Reaction::DidNotHelp) => (0, 1),
            // No reaction, or "made no difference": nothing to learn from.
            _ => continue,
        };
        let counts = by_skill.entry(outcome.capability()).or_insert((0, 0));
        counts.0 += entry.0;
        counts.1 += entry.1;
    }
    let mut ranked: Vec<_> = by_skill.into_iter().collect();
    ranked.sort_by_key(|(skill, (helped, missed))| {
        // Most-judged first; the skill name breaks ties so the prompt is stable.
        (std::cmp::Reverse(helped + missed), *skill)
    });
    ranked
        .into_iter()
        .take(MAX_SKILLS)
        .map(|(skill, (helped, missed))| match (helped, missed) {
            (h, 0) => format!("{skill} — helped {h} time(s)"),
            (0, m) => format!("{skill} — didn't help {m} time(s)"),
            (h, m) => format!("{skill} — helped {h} time(s), didn't help {m}"),
        })
        .collect()
}

/// The person says a belief is wrong: mark it corrected (drops out of understanding).
///
/// # Errors
/// [`AppError::NotFound`] if it does not exist, or [`AppError::Repository`].
pub fn correct_belief(beliefs: &impl BeliefRepository, id: BeliefId) -> Result<(), AppError> {
    let mut belief = beliefs
        .get(id)?
        .ok_or(AppError::NotFound { entity: "belief" })?;
    belief.correct();
    beliefs.save(&belief)?;
    Ok(())
}
