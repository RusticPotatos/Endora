//! Work Endora finds for itself (ADR 0094): which record to look into next, when it may
//! start, and how the record itself decides the work is over.
//!
//! All of it is a pure function of stored facts, so every rule is a unit test and none
//! needs a model. Nothing here guesses: an item is only ever a record code already derived
//! (today, a standing trouble), the source is re-read to say it is finished, and when
//! there is nothing the answer is `None`, which is a normal outcome.
use crate::goal_request::{GoalCriteria, WorkSource};
use crate::{Goal, GoalOrigin, GoalRecord, GoalStatus, GoalVerdict, Timestamp};
use endora_capabilities::StandingTrouble;
use endora_understanding::Specimen;

/// After one self-started goal ends, how long before the next may begin. Thirty minutes to
/// start with; tuned from the activity trail, not defended.
pub const COOLDOWN_AFTER_ENDING_MS: i64 = 30 * 60 * 1000;

/// How often the worker asks whether to start something. Slower than its 5-second tick:
/// starting work is not urgent, and asking is a read of the whole goal history.
pub const START_CHECK_EVERY_MS: i64 = 5 * 60 * 1000;

/// How long before the same item may be looked into again, if it is still wrong.
pub const ITEM_COOLDOWN_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// How long an ask that failed is left before it is asked again, from when it failed or was
/// last replayed. The nightly loop replayed one a night; a day keeps the same pace, and gives
/// the machinery a chance to have changed since it failed.
pub const SPECIMEN_WAIT_MS: i64 = 24 * 60 * 60 * 1000;

/// How long an ask that failed may be quoted in the goal's objective.
const ASK_CHARS: usize = 200;

/// How many times one item is looked into before Endora leaves it alone. A device that is
/// permanently broken must not be investigated forever.
pub const MAX_ATTEMPTS_PER_ITEM: usize = 2;

/// The longest server or thing name a source can hold (see `GoalCriteria::from_stored`).
const SOURCE_NAME_CHARS: usize = 200;

/// A goal's request key may be at most 200 characters; a service's name for a thing can
/// be long.
const KEY_PART_CHARS: usize = 60;

/// One thing to look into, ready to be made a goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// The goal's idempotency key. Deterministic in the item and the attempt, so a retry of
    /// the same decision is the same goal and never a second.
    pub request_key: String,
    /// What the goal is for, in words.
    pub objective: String,
    /// The record it was derived from, and what will later say it is over.
    pub source: WorkSource,
}

impl WorkItem {
    /// The goal for this item, in the lane that never outranks a request.
    ///
    /// # Errors
    /// A message if the objective or key cannot form a valid goal.
    pub fn into_record(self, at: Timestamp) -> Result<GoalRecord, &'static str> {
        let goal = Goal::for_request(
            &self.objective,
            GoalCriteria::Resolve {
                source: self.source,
            },
        )?;
        Ok(GoalRecord::new(&self.request_key, goal, at)?.with_origin(GoalOrigin::Record))
    }
}

/// A belief to look into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    /// The belief's id.
    pub id: String,
    /// What it says, in its own words.
    pub statement: String,
}

/// Everything the decision to start something rests on.
#[derive(Debug, Clone, Copy)]
pub struct Situation<'a> {
    /// What is wrong in the house right now.
    pub troubles: &'a [StandingTrouble],
    /// The asks that failed and are still worth asking again.
    pub specimens: &'a [Specimen],
    /// The belief Endora is most sure of, if there is one.
    pub focus: Option<&'a Focus>,
    /// Every goal Endora has started for itself, of any status.
    pub history: &'a [GoalRecord],
    /// Whether a goal the person asked for is open. Their work comes first.
    pub a_request_is_open: bool,
    /// Whether the person has been quiet long enough (see [`crate::record_lane`]).
    pub person_is_quiet: bool,
    /// Now.
    pub now: Timestamp,
}

/// The next thing worth looking into, or `None`.
///
/// Starts nothing unless **all** of these hold: no request of the person's is open, they have
/// been quiet, Endora's own lane is empty, and the cooldown since its last goal ended has
/// passed. Then, in order: the oldest standing trouble worth raising
/// ([`endora_capabilities::worth_raising`]) that has not been looked into too often, was not
/// stopped by the person, and is not still inside its own cooldown; else the oldest ask that
/// failed and has waited a day; else the belief it is most sure of, on the same terms as a
/// trouble. An item the person marked "it's fine" never reaches here, because `worth_raising`
/// excludes it.
#[must_use]
pub fn next_work(situation: &Situation<'_>) -> Option<WorkItem> {
    if situation.a_request_is_open || !situation.person_is_quiet {
        return None;
    }
    let now_ms = situation.now.unix_millis();
    if situation
        .history
        .iter()
        .any(|record| !record.goal().status().terminal())
    {
        return None;
    }
    let last_ended = situation
        .history
        .iter()
        .map(|record| record.updated_at().unix_millis())
        .max();
    if last_ended.is_some_and(|ended| now_ms.saturating_sub(ended) < COOLDOWN_AFTER_ENDING_MS) {
        return None;
    }
    // In this order, and only when the one before has nothing: a thing in the house that has
    // stopped answering, then an ask that failed, then something it understands about them.
    endora_capabilities::worth_raising(situation.troubles, now_ms)
        .into_iter()
        .find_map(|trouble| item_for(trouble, situation.history, now_ms))
        .or_else(|| specimen_item(situation.specimens, situation.history, now_ms))
        .or_else(|| {
            situation
                .focus
                .and_then(|focus| belief_item(focus, situation.history, now_ms))
        })
}

fn item_for(trouble: &StandingTrouble, history: &[GoalRecord], now_ms: i64) -> Option<WorkItem> {
    // A name too long to store as the source cannot be re-read later to say the work is done,
    // so it is not started. No real service names a thing this way.
    if trouble.thing.chars().count() > SOURCE_NAME_CHARS
        || trouble.server.chars().count() > SOURCE_NAME_CHARS
    {
        return None;
    }
    let stem = format!(
        "own:trouble:{}:{}:{}",
        clip(&trouble.server),
        clip(&trouble.thing),
        trouble.since_ms
    );
    // A stopped trouble comes back as a new situation with a different `since_ms`, so a
    // different stem.
    let attempt = next_attempt(&stem, history, now_ms)?;
    Some(WorkItem {
        request_key: format!("{stem}:{attempt}"),
        objective: format!("{} — find out why", trouble.statement(now_ms)),
        source: WorkSource::Trouble {
            server: trouble.server.clone(),
            thing: trouble.thing.clone(),
            since_ms: trouble.since_ms,
        },
    })
}

/// Which attempt at this item would be next, or `None` if it should be left alone: the person
/// stopped one, it has been looked into as often as it is going to be, or the last time was
/// too recent.
fn next_attempt(stem: &str, history: &[GoalRecord], now_ms: i64) -> Option<usize> {
    let attempts: Vec<&GoalRecord> = history
        .iter()
        .filter(|record| record.request_key().starts_with(&format!("{stem}:")))
        .collect();
    if attempts
        .iter()
        .any(|record| record.goal().status() == GoalStatus::Cancelled)
    {
        return None;
    }
    if attempts.len() >= MAX_ATTEMPTS_PER_ITEM {
        return None;
    }
    let last_ended = attempts
        .iter()
        .map(|record| record.updated_at().unix_millis())
        .max();
    if last_ended.is_some_and(|ended| now_ms.saturating_sub(ended) < ITEM_COOLDOWN_MS) {
        return None;
    }
    Some(attempts.len() + 1)
}

fn belief_item(focus: &Focus, history: &[GoalRecord], now_ms: i64) -> Option<WorkItem> {
    if focus.id.chars().count() > SOURCE_NAME_CHARS {
        return None;
    }
    let stem = format!("own:belief:{}", clip(&focus.id));
    let attempt = next_attempt(&stem, history, now_ms)?;
    Some(WorkItem {
        request_key: format!("{stem}:{attempt}"),
        objective: format!("Look into what would help with: {}", focus.statement),
        source: WorkSource::Belief {
            id: focus.id.clone(),
        },
    })
}

/// The oldest ask that failed and has waited long enough. How many times it has been asked is
/// the specimen's own count, not the goal history's, so it is never asked more often than the
/// nightly loop asked it; the history is consulted only for a Stop.
fn specimen_item(specimens: &[Specimen], history: &[GoalRecord], now_ms: i64) -> Option<WorkItem> {
    let touched = |s: &Specimen| s.last_replay_ms.unwrap_or(s.filed_ms);
    let mut waiting: Vec<&Specimen> = specimens
        .iter()
        .filter(|s| s.state == endora_understanding::SpecimenState::Open)
        .filter(|s| s.id.chars().count() <= SOURCE_NAME_CHARS)
        .filter(|s| now_ms.saturating_sub(touched(s)) >= SPECIMEN_WAIT_MS)
        .collect();
    waiting.sort_by_key(|s| touched(s));
    waiting.into_iter().find_map(|specimen| {
        let stem = format!("own:specimen:{}", clip(&specimen.id));
        let stopped = history.iter().any(|record| {
            record.request_key().starts_with(&format!("{stem}:"))
                && record.goal().status() == GoalStatus::Cancelled
        });
        if stopped {
            return None;
        }
        let asked: String = specimen.asked.chars().take(ASK_CHARS).collect();
        Some(WorkItem {
            request_key: format!("{stem}:{}", specimen.replays + 1),
            objective: format!("Ask again: \"{}\"", asked.trim()),
            source: WorkSource::Specimen {
                id: specimen.id.clone(),
            },
        })
    })
}

fn clip(text: &str) -> String {
    text.chars().take(KEY_PART_CHARS).collect()
}

/// What re-reading the source says about work already under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recheck {
    /// The condition still holds; carry on.
    Unchanged,
    /// The condition no longer holds. The goal, completed by the source and not by any model.
    Resolved(Box<GoalRecord>),
    /// The person said it is fine. The work is stopped, with the reason.
    Withdrawn(String),
}

/// The stored records a source is checked against.
#[derive(Debug, Clone, Copy)]
pub struct Records<'a> {
    /// What is wrong in the house now.
    pub troubles: &'a [StandingTrouble],
    /// The asks still open.
    pub open_specimens: &'a [Specimen],
    /// The ids of the beliefs still held.
    pub active_beliefs: &'a [String],
}

/// Re-reads the record a self-started goal was derived from.
///
/// **The source is the verifier** (ADR 0094): a model never declares this work finished.
/// Only a goal that resolves from a record, still open, is ever changed here. A trouble that
/// cleared completes it. An ask no longer on the shelf, or a belief no longer held, withdraws
/// it: the thing it was for is gone, and nothing was answered.
#[must_use]
pub fn recheck(record: &GoalRecord, records: &Records<'_>, at: Timestamp) -> Recheck {
    let Some(GoalCriteria::Resolve { source }) = record.goal().criteria() else {
        return Recheck::Unchanged;
    };
    if record.goal().status().terminal() {
        return Recheck::Unchanged;
    }
    match source {
        WorkSource::Trouble {
            server,
            thing,
            since_ms,
        } => {
            let current = records
                .troubles
                .iter()
                .find(|trouble| &trouble.server == server && &trouble.thing == thing);
            match current {
                Some(trouble) if trouble.since_ms == *since_ms && trouble.accepted => {
                    Recheck::Withdrawn(format!("you said {thing} is fine"))
                }
                Some(trouble) if trouble.since_ms == *since_ms => Recheck::Unchanged,
                // Gone, or gone and back since: either way the situation it was started for
                // is over.
                _ => {
                    let mut goal = record.goal().clone();
                    goal.start("checking the source", "read the record again");
                    goal.commit(
                        "checking the source",
                        &format!(
                            "{thing} is answering again, so there is nothing left to find out."
                        ),
                    );
                    goal.apply(
                        GoalVerdict::Complete,
                        "the record it was started from no longer says it is wrong",
                    );
                    Recheck::Resolved(Box::new(record.with_goal(goal, at)))
                }
            }
        }
        WorkSource::Specimen { id } => {
            if records.open_specimens.iter().any(|s| &s.id == id) {
                Recheck::Unchanged
            } else {
                Recheck::Withdrawn("that question is no longer on my list".to_owned())
            }
        }
        WorkSource::Belief { id } => {
            if records.active_beliefs.contains(id) {
                Recheck::Unchanged
            } else {
                Recheck::Withdrawn("I no longer hold what this was about".to_owned())
            }
        }
    }
}

/// One line for the activity trail when self-started work ends. Said in the first person and
/// plainly: it is the only place the person learns what happened, since this lane posts no
/// chat message.
#[must_use]
pub fn trail_line(record: &GoalRecord) -> String {
    // A replay says what became of the question in the words the nightly loop always used
    // ("A question that once stumped me answers now"), so the trail reads the same whichever
    // path asked it.
    if let Some(GoalCriteria::Resolve {
        source: WorkSource::Specimen { .. },
    }) = record.goal().criteria()
    {
        if let Some(found) = record.goal().evidence().last() {
            return found.output.clone();
        }
    }
    let notice = record.goal().outcome_notice();
    let notice: String = notice.split_whitespace().collect::<Vec<_>>().join(" ");
    let notice: String = notice.chars().take(500).collect();
    format!(
        "On my own I looked into \"{}\" ({}): {}",
        record.goal().objective(),
        record.goal().status().name(),
        notice
    )
}

#[cfg(test)]
mod tests;
