//! Use cases — the butler turn and the flows around it.
//!
//! These orchestrate the contexts and the ports. They are the seam the interfaces
//! (node, CLI) call; they hold no transport or storage detail. Identifiers and
//! time come from the [`IdSource`] and [`Clock`] ports, so the domain stays pure
//! and the use cases stay testable with fakes.

use endora_conversation::{ChatMessage, MessageRole};
use endora_kernel::ids::{AuditId, BeliefId, MessageId, OutcomeId, PreferenceId, Timestamp};
use endora_kernel::{Decision, Reversibility};
use endora_platform::AuditRecord;
use endora_understanding::{
    Intention, Outcome, Preference, PreferenceKind, Reaction, RepairProposal,
};

use endora_capabilities::{CapabilityRunner, CapabilitySpec};
use endora_conversation::ChatRepository;
use endora_platform::{AuditLog, EventLog};
use endora_understanding::{Brain, IntentionRepository, OutcomeRepository, PreferenceRepository};

use endora_scheduling::{BriefSchedule, BriefScheduleRepository};

use crate::error::AppError;
use crate::ports::{
    Butler, ButlerContext, ButlerReply, CapabilityTool, Clock, ConversationSummary,
    ConversationSummaryStore, FormedBelief, IdSource, MemorySnapshot, MemoryStore, TurnMessage,
};

/// Exports everything the user has stored (a memory right).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn export_memory(store: &impl MemoryStore) -> Result<MemorySnapshot, AppError> {
    Ok(store.export()?)
}

/// Permanently deletes all of the user's stored data (a memory right).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn purge_memory(store: &impl MemoryStore) -> Result<(), AppError> {
    Ok(store.purge()?)
}

/// Returns the most recent audit records, newest first, up to `limit`.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
// `recent_audit` moved to the platform context (ADR 0050); re-exported so
// `usecases::recent_audit` paths are unchanged during the migration.
pub use endora_platform::recent_audit;

/// What kind of thing an [`ActivityItem`] records.
///
/// Kept coarse on purpose: the feed groups by what sort of event it was, and the
/// human-readable summary carries the specifics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    /// A consequential decision was made and audited (see [`AuditLog`]).
    Decision,
    /// Something the butler did or learned this turn, or a setting the person
    /// changed — the butler's own action log (see [`EventLog`]).
    Action,
}

impl ActivityKind {
    /// A stable, lowercase name, suitable for the protocol and the UI.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Action => "action",
        }
    }
}

/// One entry in the activity feed.
///
/// This is a **read projection**, not a domain aggregate: it merges the
/// persisted facts that already carry a time — audited decisions and the butler's
/// own action log — into a single "what happened" timeline. Because it is derived,
/// it stores nothing new and needs no schema of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityItem {
    at: Timestamp,
    kind: ActivityKind,
    summary: String,
}

impl ActivityItem {
    /// When the recorded event happened.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The area of the loop this entry belongs to.
    #[must_use]
    pub const fn kind(&self) -> ActivityKind {
        self.kind
    }

    /// The human-readable description of what happened.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

/// Returns the most recent activity, newest first, up to `limit` entries.
///
/// The feed is a projection over what is already persisted with a timestamp:
/// audited decisions and the butler's own action log. As more of the system gains
/// durable timestamps, this timeline widens without a protocol change.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn recent_activity(
    audit: &impl AuditLog,
    events: &impl EventLog,
    limit: usize,
) -> Result<Vec<ActivityItem>, AppError> {
    let mut items = Vec::new();
    for r in audit.recent(limit)? {
        items.push(ActivityItem {
            at: r.at(),
            kind: ActivityKind::Decision,
            summary: r.summary().to_owned(),
        });
    }
    for e in events.recent(limit)? {
        items.push(ActivityItem {
            at: e.at,
            kind: ActivityKind::Action,
            summary: e.summary,
        });
    }
    // Newest first; break ties stably so equal timestamps keep a deterministic
    // order across calls.
    items.sort_by(|a, b| {
        b.at.unix_millis()
            .cmp(&a.at.unix_millis())
            .then_with(|| b.summary.cmp(&a.summary))
    });
    items.truncate(limit);
    Ok(items)
}
mod turn;
pub use turn::*;

mod beliefs;
pub use beliefs::*;

mod proactive;
pub use proactive::*;

mod mail;
pub use mail::*;

mod agenda;
pub use agenda::*;

mod clock;
pub use clock::*;

mod context;
pub use context::*;

mod brain;
pub use brain::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod what_a_read_gets_back;

#[cfg(test)]
mod one_call_two_spellings;

#[cfg(test)]
mod why_it_could_not_have_worked;

#[cfg(test)]
mod facts_only_where_they_mean_something;

#[cfg(test)]
mod what_reaches_an_empty_room;

/// How far back "while I was out" reaches, when nobody says.
///
/// A day, because the question is asked on returning and the answer people want is "since I
/// last paid attention", not "since the beginning".
pub const A_DAY_MS: i64 = 86_400_000;

/// At most this many of any one kind of thing, so the answer stays an answer.
const ENOUGH_OF_ANY_ONE_THING: usize = 5;

/// What Endora has actually been doing, read from its own record (ADR 0056).
///
/// Asked "did you do anything while I was out?" and then "nothing proactive done today?",
/// the butler replied **"No specific activities were recorded today"** — on a day it had
/// posted three unprompted messages, one of them a real morning brief four hours earlier.
/// Every part of the true answer was stored; none of it was reachable from a turn, so the
/// most important question a proactive butler can be asked was the one it could not answer.
///
/// Being proactive and unable to account for it is worse than not being proactive: the
/// person cannot tell a quiet day from a broken one.
///
/// Deterministic, and assembled here rather than asked of the model — this is a **report of
/// stored facts**, which is the one thing Endora is entitled to assert
/// ([0053](../../docs/adr/0053-honesty-about-what-it-did.md)). The model's job is to say it
/// nicely, not to remember it.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
pub fn what_it_has_been_doing(
    chat: &impl ChatRepository,
    outcomes: &impl OutcomeRepository,
    writes: &impl endora_capabilities::ConfigWriteLog,
    since_ms: i64,
    now_ms: i64,
) -> Result<String, AppError> {
    let mut lines: Vec<String> = Vec::new();

    // Messages Endora started itself. There is no stored flag for "proactive", so it is
    // read from adjacency: a butler message whose predecessor is not the person's is one
    // nobody asked for. A butler message at the very start of the window has no visible
    // predecessor and is counted, which is right far more often than not — the alternative
    // is silently dropping the brief that opened the day.
    let history = chat.between(since_ms, now_ms)?;
    let mut spoke_first: Vec<&ChatMessage> = Vec::new();
    for (i, message) in history.iter().enumerate() {
        if message.role() != MessageRole::Butler {
            continue;
        }
        let asked_for = i > 0 && history[i - 1].role() == MessageRole::User;
        if !asked_for {
            spoke_first.push(message);
        }
    }
    for message in spoke_first.iter().take(ENOUGH_OF_ANY_ONE_THING) {
        lines.push(format!(
            "{} — I wrote to you unprompted: \"{}\"",
            clock_time_of(message.at().unix_millis()),
            first_sentence_of(message.text())
        ));
    }

    // What it did, kept as claim-and-observation rather than a verdict, exactly as stored.
    let acted: Vec<_> = outcomes
        .list()?
        .into_iter()
        .filter(|o| o.at().unix_millis() >= since_ms)
        .collect();
    let (worked, failed): (Vec<_>, Vec<_>) = acted
        .iter()
        .partition(|o| !o.claim().trim_start().starts_with("error:"));
    for outcome in worked.iter().take(ENOUGH_OF_ANY_ONE_THING) {
        let verdict = match outcome.changed() {
            Some(true) => "and I saw it change",
            Some(false) => "but nothing changed when I looked",
            None => "though I could not check",
        };
        lines.push(format!(
            "{} — used {} {verdict}",
            clock_time_of(outcome.at().unix_millis()),
            outcome.capability()
        ));
    }
    for outcome in failed.iter().take(ENOUGH_OF_ANY_ONE_THING) {
        lines.push(format!(
            "{} — tried {} and it failed",
            clock_time_of(outcome.at().unix_millis()),
            outcome.capability()
        ));
    }

    // Changes it made inside somebody else's service, which are the consequential ones.
    for write in writes
        .writes(ENOUGH_OF_ANY_ONE_THING)?
        .iter()
        .filter(|w| w.at_ms >= since_ms)
    {
        lines.push(format!(
            "{} — changed {} in {}",
            clock_time_of(write.at_ms),
            write.target,
            write.server
        ));
    }

    if lines.is_empty() {
        // The honest empty answer, and deliberately not "no activities were recorded".
        // Nothing happening and nothing being *recorded* are different claims, and the
        // second one was made falsely.
        return Ok(
            "Nothing. I did not write to you, act on anything, change any settings, or \
             notice anything stop answering in that time."
                .to_owned(),
        );
    }
    lines.sort();
    Ok(lines.join("\n"))
}

/// `HH:MM` in UTC, matching how the rest of the system reports times.
fn clock_time_of(ms: i64) -> String {
    let secs = ms.rem_euclid(A_DAY_MS) / 1000;
    format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

/// Whether the question is about something out in the world, answerable only by looking it
/// up: when something is, what is next, a score, a schedule, a price. Words, not judgement,
/// and the same bargain as [`says_it_found_nothing`]: a wrong yes costs one deep-model call.
/// A question about the person's own things ("my", "our") is never one, so it stays local.
#[must_use]
pub fn asks_about_the_world(text: &str) -> bool {
    let words = words_of(text);
    let has = |wanted: &str| words.iter().any(|w| w == wanted);
    if ["my", "mine", "our", "ours", "i", "we", "me"]
        .iter()
        .any(|w| has(w))
    {
        return false;
    }
    [
        "when",
        "next",
        "latest",
        "score",
        "scores",
        "schedule",
        "schedules",
        "price",
        "prices",
        "playing",
        "showtimes",
        "forecast",
        "standings",
    ]
    .iter()
    .any(|w| has(w))
}

/// Whether a message only asks the date, the day or the time.
fn asks_the_clock(text: &str) -> bool {
    let lowered = text.to_lowercase().replace('\u{2019}', "'");
    let asked = lowered.trim().trim_end_matches(['?', '.', '!']).trim();
    [
        "what day is it",
        "what day is today",
        "what is today",
        "what's today",
        "what's the date",
        "what is the date",
        "what's today's date",
        "what is today's date",
        "what date is it",
        "what time is it",
        "what's the time",
        "what is the time",
        "what's the date today",
        "what is the date today",
    ]
    .contains(&asked)
}

/// Whether a question needs a look at the outside world (ADR 0089). The local model sorts it
/// (`classify_need`: one cheap call, nothing leaves the device); only when it cannot say do the
/// word lists decide.
fn needs_a_lookup(butler: &dyn Butler, text: &str) -> bool {
    // The date and time are in every request, so asking them is answered, not looked up,
    // whatever the classifier makes of "today".
    if asks_the_clock(text) {
        return false;
    }
    match butler.classify_need(text) {
        Some(crate::NeedKind::Lookup) => true,
        Some(_) => false,
        None => asks_about_the_world(text),
    }
}

/// The reply with any copy of the "what I did" record taken out, unless the person asked for
/// it. The record is appended by code when they do (`account_behind`); when they did not, a
/// model quoting it back is a leak of its own instructions, and everything from the marker on
/// is the copy.
fn without_the_record_unless_asked(reply: &str, asked: &str) -> String {
    if asks_what_it_did(asked) {
        return reply.to_owned();
    }
    match reply.find("[did]") {
        Some(0) => String::new(),
        Some(at) => reply[..at].trim_end().to_owned(),
        None => reply.to_owned(),
    }
}

/// Whether a reply says it looked and found nothing. Words, not judgement: it only decides
/// whether to ask once more, and a wrong yes costs a deep-model call, never a wrong answer.
fn says_it_found_nothing(text: &str) -> bool {
    const PHRASES: [&str; 12] = [
        "couldn't find",
        "could not find",
        "can't find",
        "cannot find",
        "unable to find",
        "not able to find",
        "didn't find",
        "did not find",
        "no upcoming",
        "no results",
        "nothing found",
        "no ticketed",
    ];
    let lowered = text.to_lowercase().replace('\u{2019}', "'");
    PHRASES.iter().any(|phrase| lowered.contains(phrase))
}

/// The first sentence, so a quoted brief does not become the whole report.
fn first_sentence_of(text: &str) -> String {
    const ENOUGH: usize = 90;
    let trimmed = text.trim();
    let end = trimmed
        .find(['.', '!', '?'])
        .map_or(trimmed.len(), |i| i + 1);
    let cut = end.min(ENOUGH);
    let mut out: String = trimmed.chars().take(cut).collect();
    if cut < trimmed.chars().count() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod accounting_for_itself;

#[cfg(test)]
mod the_record_next_to_the_claim;

#[cfg(test)]
mod asking_again_is_an_answer;

#[cfg(test)]
mod when_the_local_model_will_not_do_it;

#[cfg(test)]
mod a_night_that_did_nothing;

#[cfg(test)]
mod its_own_behaviour_is_not_evidence_about_you;

#[cfg(test)]
mod who_words_the_brief;

#[cfg(test)]
mod one_fact_source_reaches_everything;

#[cfg(test)]
mod what_a_stranger_said;

#[cfg(test)]
mod what_stays_home;

#[cfg(test)]
mod where_they_are_is_a_fact;

#[cfg(test)]
mod escalation_is_a_habit;

#[cfg(test)]
mod it_keeps_its_own_score;

#[cfg(test)]
mod the_record_graduates_a_tool;

#[cfg(test)]
mod asking_again_does_not_start_from_nothing;

#[cfg(test)]
mod what_the_turn_is_offered;
