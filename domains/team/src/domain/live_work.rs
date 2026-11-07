//! The Live Work map (proposal v3 §14; the owner's decision M3, 7 Nov 2026): one work item's
//! path as HQ recorded it, its state now, its handoffs, its evidence and what waits on the
//! owner. A read-only projection of records HQ already keeps. Nothing here guesses: a state is
//! shown only when the record says so, "completed" only with a verification, and a stretch where
//! nothing moved is called a stall only when the record shows wakes turned away in it.

use serde_json::{Value, json};

use super::work::Millis;

const MINUTE: Millis = 60_000;
const HOUR: Millis = 60 * MINUTE;
const DAY: Millis = 24 * HOUR;

/// A running turn with no sign of life for this long is shown as "no signal since", never busy.
pub const FRESH_FOR: Millis = 20 * MINUTE;

/// A gap with nothing recorded, this long or longer, is a stall when wakes were refused in it.
pub const STALL_AFTER: Millis = 6 * HOUR;

/// What HQ recorded about one item, as the map reads it. Names are members' names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Seen {
    /// The time now.
    pub now: Millis,
    /// The item's status name: `todo`, `doing`, `in_review`, `blocked`, `done`, `dropped`.
    pub status: String,
    /// Whom it is assigned to.
    pub assignee: Option<String>,
    /// Why it closed, if it did.
    pub closure: Option<ClosureSeen>,
    /// Its turns, oldest first.
    pub runs: Vec<RunSeen>,
    /// The wakes on it that were turned away.
    pub refused: Vec<RefusedSeen>,
    /// The branches submitted for it.
    pub branches: Vec<BranchSeen>,
    /// The checks run on those branches.
    pub checks: Vec<CheckSeen>,
    /// The desk cards about it.
    pub desk: Vec<DeskSeen>,
    /// Its handover notes.
    pub handovers: Vec<HandoverSeen>,
    /// The latest sign of life from its running turn (a call or a post), if any.
    pub last_signal: Option<Millis>,
}

/// A closure as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureSeen {
    /// Its reason's name: `met_criteria`, `completed_unverified`, `duplicate`, …
    pub why: String,
    /// The reason in plain words.
    pub said: String,
    /// When.
    pub at: Millis,
}

/// A turn as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSeen {
    /// Its id.
    pub id: i64,
    /// Whose turn.
    pub who: String,
    /// Why it was woken.
    pub why: String,
    /// `queued`, `running`, `retry_waiting`, `succeeded`, `failed`, `stopped`.
    pub status: String,
    /// When it started.
    pub started: Option<Millis>,
    /// When it ended.
    pub ended: Option<Millis>,
    /// How it ended, in HQ's words.
    pub end_reason: String,
    /// How many calls it made.
    pub receipts: usize,
    /// Whether it moved the work forward.
    pub moved_forward: bool,
}

/// A wake turned away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedSeen {
    /// When.
    pub at: Millis,
    /// For whom.
    pub who: String,
    /// Why it was raised.
    pub reason: String,
    /// Why it was refused: `out_of_budget`, `in_review`, …
    pub refused: String,
}

/// A branch submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSeen {
    /// Its name.
    pub name: String,
    /// Files changed.
    pub files: usize,
    /// Lines added.
    pub plus: i64,
    /// Lines removed.
    pub minus: i64,
    /// When.
    pub at: Millis,
    /// The turn that submitted it.
    pub run: i64,
    /// Who submitted it.
    pub who: String,
}

/// A check on a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckSeen {
    /// Its id.
    pub id: i64,
    /// Passed, failed, or not back yet.
    pub ok: Option<bool>,
    /// What it found.
    pub summary: String,
    /// When it came back (or was queued, while it runs).
    pub at: Millis,
    /// The turn that asked for it.
    pub run: i64,
}

/// A desk card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskSeen {
    /// Its id.
    pub id: i64,
    /// Its kind: `decision`, `question`, `stuck`, `guard`, `push`, `deploy`, `deploy_check`, …
    pub kind: String,
    /// What the owner is shown.
    pub text: String,
    /// `waiting`, or how it was decided.
    pub status: String,
    /// When it was raised.
    pub at: Millis,
    /// The member who raised it, if one did: the handoff to the owner.
    pub from: Option<String>,
}

/// A handover note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverSeen {
    /// Who held it.
    pub from: Option<String>,
    /// Whom it went to.
    pub to: Option<String>,
    /// When.
    pub at: Millis,
}

/// The item's state now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// A turn runs and was heard from recently.
    Working,
    /// It waits: on the owner, a queued turn, or a wake.
    Waiting,
    /// A check failed, or HQ marked it blocked.
    Blocked,
    /// A turn runs but has not been heard from within [`FRESH_FOR`].
    Unknown,
    /// Done, with a verification on record.
    Completed,
    /// Done, with nothing that verified it.
    DoneUnverified,
    /// Dropped or closed without doing it.
    Closed,
}

impl State {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
            Self::Completed => "completed",
            Self::DoneUnverified => "done_unverified",
            Self::Closed => "closed",
        }
    }
}

/// How a step is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// It went as it should.
    Ok,
    /// It failed.
    Bad,
    /// Stalled, or raised by HQ itself.
    Warn,
    /// The owner acted, or must.
    Owner,
    /// Neither good nor bad.
    Calm,
    /// Unresolved: nothing says how it ended.
    Open,
}

impl Tone {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Bad => "bad",
            Self::Warn => "warn",
            Self::Owner => "owner",
            Self::Calm => "calm",
            Self::Open => "open",
        }
    }
}

/// One step of the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What kind: `assigned`, `submitted`, `check`, `desk`, `stall`, `handover`, `closed`.
    pub kind: &'static str,
    /// When it happened (a stall: when it began).
    pub at: Millis,
    /// When a stall ended.
    pub until: Option<Millis>,
    /// How it is drawn.
    pub tone: Tone,
    /// What happened, in a line.
    pub label: String,
    /// A second line, if any.
    pub sub: String,
    /// The records it rests on: `c13`, `#257`, `run 2318`.
    pub ids: Vec<String>,
    /// How the owner decided, for a desk card the owner answered.
    pub owner: Option<String>,
    /// The turns that ended since the step before.
    pub runs: Vec<i64>,
}

/// Someone who took a turn on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    /// The member's name.
    pub name: String,
    /// Turns taken.
    pub turns: usize,
    /// Of those, how many failed.
    pub failed: usize,
}

/// The map of one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveMap {
    /// Its state now.
    pub state: State,
    /// The state in plain words.
    pub said: String,
    /// When its running turn was last heard from, while unknown.
    pub silent_since: Option<Millis>,
    /// The desk cards waiting on the owner, first.
    pub attention: Vec<DeskSeen>,
    /// Steps left unresolved (an inconclusive check), said apart.
    pub open: Vec<String>,
    /// The path, in time order.
    pub steps: Vec<Step>,
    /// Who took part.
    pub people: Vec<Person>,
}

/// The map of one item from its record.
#[must_use]
pub fn live_map(seen: &Seen) -> LiveMap {
    let mut steps = milestones(seen);
    steps.sort_by_key(|s| s.at);
    if let Some(stall) = stall(seen, &steps) {
        let at = steps.partition_point(|s| s.at <= stall.at);
        steps.insert(at, stall);
    }
    attach_runs(&mut steps, &seen.runs);
    let (state, said, silent_since) = state(seen);
    LiveMap {
        state,
        said,
        silent_since,
        attention: seen
            .desk
            .iter()
            .filter(|d| d.status == "waiting")
            .cloned()
            .collect(),
        open: steps
            .iter()
            .filter(|s| s.tone == Tone::Open)
            .map(|s| s.label.clone())
            .collect(),
        steps,
        people: people(&seen.runs),
    }
}

fn step(kind: &'static str, at: Millis, tone: Tone, label: String) -> Step {
    Step {
        kind,
        at,
        until: None,
        tone,
        label,
        sub: String::new(),
        ids: Vec::new(),
        owner: None,
        runs: Vec::new(),
    }
}

fn milestones(seen: &Seen) -> Vec<Step> {
    let mut steps = Vec::new();
    let first = seen.runs.iter().filter_map(|r| r.started.or(r.ended)).min();
    if let Some(at) = first {
        let who = seen.assignee.clone().unwrap_or_else(|| "nobody".into());
        steps.push(step("assigned", at, Tone::Ok, format!("Assigned to {who}")));
    }
    for b in &seen.branches {
        let mut s = step(
            "submitted",
            b.at,
            Tone::Ok,
            format!("{} submitted the branch", b.who),
        );
        s.sub = format!("{} files, +{} −{}", b.files, b.plus, b.minus);
        s.ids = vec![b.name.clone(), format!("run {}", b.run)];
        steps.push(s);
    }
    for c in &seen.checks {
        let (tone, word) = match c.ok {
            Some(true) => (Tone::Ok, "passed"),
            Some(false) => (Tone::Bad, "failed"),
            None => (Tone::Calm, "running"),
        };
        let mut s = step("check", c.at, tone, format!("Check c{} {word}", c.id));
        s.sub.clone_from(&c.summary);
        s.ids = vec![format!("c{}", c.id), format!("run {}", c.run)];
        steps.push(s);
    }
    steps.extend(seen.desk.iter().map(desk_step));
    for h in &seen.handovers {
        let (Some(from), Some(to)) = (&h.from, &h.to) else {
            continue;
        };
        if from != to {
            steps.push(step(
                "handover",
                h.at,
                Tone::Calm,
                format!("{from} handed it to {to}"),
            ));
        }
    }
    if let Some(c) = &seen.closure {
        let (tone, label) = match (seen.status.as_str(), c.why.as_str()) {
            (_, "completed_unverified") => (Tone::Open, "Done, not verified".to_owned()),
            ("done", _) => (Tone::Ok, format!("Done: {}", c.said)),
            _ => (Tone::Calm, format!("Closed: {}", c.said)),
        };
        steps.push(step("closed", c.at, tone, label));
    }
    steps
}

/// A desk card as a step. Who raised it decides its colour: a card HQ raised on its own (stuck,
/// a guard) is not the owner's decision, and the owner's word that it is deployed is not a check.
fn desk_step(d: &DeskSeen) -> Step {
    let waiting = d.status == "waiting";
    let decided = match d.status.as_str() {
        "waiting" => "Waiting on you".to_owned(),
        "approve" | "approved" => "Approved".to_owned(),
        "decline" | "declined" => "Declined".to_owned(),
        "retry" => "Chose retry".to_owned(),
        "pushed" => "Approved push".to_owned(),
        "deployed" => "Said Deployed".to_owned(),
        other => capitalised(&other.replace('_', " ")),
    };
    let (tone, label) = match d.kind.as_str() {
        "stuck" => (Tone::Warn, "Flagged stuck".to_owned()),
        "guard" => (Tone::Warn, "Guard raised".to_owned()),
        "push" => (Tone::Owner, "Push to the fork".to_owned()),
        "deploy" if d.status == "deployed" => {
            (Tone::Owner, "Deployed (the owner's word)".to_owned())
        }
        "deploy" => (Tone::Owner, "Deploy".to_owned()),
        "deploy_check" => post_deploy(&d.text),
        kind => (Tone::Owner, capitalised(&kind.replace('_', " "))),
    };
    let label = match &d.from {
        Some(from) if matches!(d.kind.as_str(), "decision" | "question") => {
            format!("{label} from {from}")
        }
        _ => label,
    };
    let mut s = step(
        "desk",
        d.at,
        if waiting { Tone::Owner } else { tone },
        label,
    );
    s.sub = first_line(&d.text);
    s.ids = vec![format!("#{}", d.id)];
    // "Seen" is not a decision: a post-deploy check's card is only read.
    if d.kind != "deploy_check" || waiting {
        s.owner = Some(decided);
    }
    s
}

/// What a post-deploy check found, from its card's own words; anything else is left open.
fn post_deploy(text: &str) -> (Tone, String) {
    let lower = text.to_lowercase();
    if lower.contains("inconclusive") {
        return (Tone::Open, "Post-deploy check inconclusive".to_owned());
    }
    if lower.contains("failed") || lower.contains("unhealthy") {
        return (Tone::Bad, "Post-deploy check failed".to_owned());
    }
    if lower.contains("passed") || lower.contains("healthy") {
        return (Tone::Ok, "Post-deploy check passed".to_owned());
    }
    (Tone::Open, "Post-deploy check: no verdict".to_owned())
}

/// The longest stretch with nothing recorded, when it is long and wakes were turned away while
/// the work waited: since the last step that moved it, up to the gap's end.
fn stall(seen: &Seen, steps: &[Step]) -> Option<Step> {
    let mut times: Vec<Millis> = steps.iter().map(|s| s.at).collect();
    times.extend(seen.refused.iter().map(|r| r.at));
    times.extend(
        seen.runs
            .iter()
            .flat_map(|r| [r.started, r.ended])
            .flatten(),
    );
    times.sort_unstable();
    let (start, end) = times
        .windows(2)
        .map(|w| (w[0], w[1]))
        .filter(|(a, b)| b - a >= STALL_AFTER)
        .max_by_key(|(a, b)| b - a)?;
    let moved = steps
        .iter()
        .filter(|s| {
            s.at <= start && matches!(s.kind, "assigned" | "submitted" | "check" | "handover")
        })
        .map(|s| s.at)
        .max()
        .unwrap_or(Millis::MIN);
    let refused: Vec<&RefusedSeen> = seen
        .refused
        .iter()
        .filter(|r| r.at > moved && r.at <= end)
        .collect();
    if refused.is_empty() {
        return None;
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    for r in &refused {
        let word = r.refused.replace('_', " ");
        match counts.iter_mut().find(|(w, _)| *w == word) {
            Some((_, n)) => *n += 1,
            None => counts.push((word, 1)),
        }
    }
    let top = counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(w, _)| w.clone())
        .unwrap_or_default();
    let span = end - start;
    let long = if span >= DAY {
        format!("{} days", (span + DAY / 2) / DAY)
    } else {
        format!("{} h", (span + HOUR / 2) / HOUR)
    };
    let mut s = step("stall", start, Tone::Warn, format!("{long} stalled: {top}"));
    s.until = Some(end);
    let parts: Vec<String> = counts.iter().map(|(w, n)| format!("{n} {w}")).collect();
    s.sub = format!("{} wakes refused: {}", refused.len(), parts.join(", "));
    Some(s)
}

fn attach_runs(steps: &mut [Step], runs: &[RunSeen]) {
    let mut before = Millis::MIN;
    for s in steps.iter_mut() {
        s.runs = runs
            .iter()
            .filter(|r| r.ended.is_some_and(|e| e > before && e <= s.at))
            .map(|r| r.id)
            .collect();
        before = s.at;
    }
}

fn state(seen: &Seen) -> (State, String, Option<Millis>) {
    if let Some(c) = &seen.closure {
        if c.why == "completed_unverified" {
            return (
                State::DoneUnverified,
                "Done, not verified: nothing on record confirmed it".into(),
                None,
            );
        }
        if seen.status == "done" {
            return (
                State::Completed,
                "Done, verified against its criteria".into(),
                None,
            );
        }
        return (State::Closed, format!("Closed: {}", c.said), None);
    }
    match seen.status.as_str() {
        "done" => {
            return (
                State::DoneUnverified,
                "Done, not verified: nothing on record confirmed it".into(),
                None,
            );
        }
        "dropped" => return (State::Closed, "Closed".into(), None),
        _ => {}
    }
    if let Some(d) = seen.desk.iter().find(|d| d.status == "waiting") {
        return (
            State::Waiting,
            format!("Waiting on you: {}", first_line(&d.text)),
            None,
        );
    }
    if let Some(r) = seen.runs.iter().find(|r| r.status == "running") {
        let heard = seen
            .last_signal
            .into_iter()
            .chain(r.started)
            .max()
            .unwrap_or(seen.now);
        if seen.now - heard > FRESH_FOR {
            return (
                State::Unknown,
                "No signal from the running turn".into(),
                Some(heard),
            );
        }
        return (
            State::Working,
            format!("Working: {}'s turn is running", r.who),
            None,
        );
    }
    if seen
        .runs
        .iter()
        .any(|r| matches!(r.status.as_str(), "queued" | "retry_waiting"))
    {
        return (State::Waiting, "Waiting: a turn is queued".into(), None);
    }
    let last_check = seen.checks.iter().max_by_key(|c| c.at);
    if let Some(c) = last_check.filter(|c| c.ok == Some(false)) {
        return (
            State::Blocked,
            format!("Blocked: check c{} failed", c.id),
            None,
        );
    }
    if seen.status == "blocked" {
        return (State::Blocked, "Blocked".into(), None);
    }
    (State::Waiting, "Waiting: no turn is queued".into(), None)
}

fn people(runs: &[RunSeen]) -> Vec<Person> {
    let mut people: Vec<Person> = Vec::new();
    for r in runs.iter().filter(|r| r.started.is_some()) {
        let failed = usize::from(r.status == "failed");
        match people.iter_mut().find(|p| p.name == r.who) {
            Some(p) => {
                p.turns += 1;
                p.failed += failed;
            }
            None => people.push(Person {
                name: r.who.clone(),
                turns: 1,
                failed,
            }),
        }
    }
    people
}

/// The time between two steps, as the strip says it: `+8 min`, `+3 h`, `+7 d`.
#[must_use]
pub fn gap(ms: Millis) -> String {
    if ms < MINUTE {
        return "<1 min".into();
    }
    if ms < HOUR {
        return format!("+{} min", ms / MINUTE);
    }
    if ms < DAY {
        return format!("+{} h", ms / HOUR);
    }
    format!("+{} d", ms / DAY)
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    super::evidence::short(line, 160)
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

impl LiveMap {
    /// The map as the console reads it.
    #[must_use]
    pub fn shown(&self) -> Value {
        let mut before: Option<Millis> = None;
        let steps: Vec<Value> = self
            .steps
            .iter()
            .map(|s| {
                let cue = before.map(|b| gap(s.at - b));
                before = Some(s.until.unwrap_or(s.at));
                json!({ "kind": s.kind, "at": s.at, "until": s.until, "tone": s.tone.name(),
                        "label": s.label, "sub": s.sub, "ids": s.ids, "owner": s.owner,
                        "runs": s.runs, "gap": cue })
            })
            .collect();
        json!({
            "state": self.state.name(), "said": self.said, "silent_since": self.silent_since,
            "attention": self.attention.iter().map(|d| json!({ "id": d.id, "kind": d.kind,
                "text": first_line(&d.text), "at": d.at })).collect::<Vec<_>>(),
            "open": self.open, "steps": steps,
            "people": self.people.iter().map(|p| json!({ "name": p.name, "turns": p.turns,
                "failed": p.failed })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
#[path = "live_work_tests.rs"]
mod tests;
