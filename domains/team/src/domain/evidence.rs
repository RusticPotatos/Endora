//! Evidence (ADR 0114): what shows that work is done, and what a turn did, from the record
//! rather than from what a model says.
//!
//! - A **receipt** is the node's record of one call a turn made of Endora's tools (a reading
//!   such as `homelab_status`, or an act such as `restart_container`): written by the node,
//!   never reported by the model, so a model cannot forge one.
//! - A **sandbox-reported** receipt (ADR 0117) is what a turn's sandbox said its own tools did
//!   (Claude Code's Bash or Edit, posted by a hook): recorded by the node with the turn's token,
//!   but not witnessed by it. It may be cited; a live criterion still needs a node-witnessed
//!   reading.
//! - A task's **acceptance criteria** say what "done" means. A criterion about the lab as it
//!   is now is **live**: only a reading taken in the same turn shows it.
//! - **One evidence rule** ([`judge`]) decides every claim that work is done or a decision is
//!   founded: each criterion needs at least one reference that is in HQ's record and belongs to
//!   this work, and a live one a fresh reading.
//! - A turn's **label** ([`label`]) is worked out from what happened in it, never from its words.

use super::work::{Millis, Refusal, RunId, TaskId};

/// The most of a result a receipt keeps, in characters.
pub const EXCERPT_MOST: usize = 300;
/// The most of a call's arguments a receipt keeps, in characters.
pub const ARGS_MOST: usize = 120;
/// The most a criterion says, in characters.
pub const CRITERION_MOST: usize = 200;
/// The most criteria a task has.
pub const MOST_CRITERIA: usize = 8;
/// The most references one claim cites.
pub const MOST_REFS: usize = 30;
/// How many turns of a member's on a task that show nothing are nudged before the task goes up
/// to whom the member reports to.
pub const MOST_NUDGES: usize = 2;
/// How many times work may be sent back before its next submission goes to the owner.
pub const MOST_ROUNDS: usize = 3;

/// What a member whose last turn showed nothing is told at the start of the next.
pub const NUDGE: &str = "Your last turn on this task showed nothing: take the first concrete \
    action now (a reading, a change, or evidence against a criterion), or mark the task blocked \
    and say why.";

/// The criteria every home-lab routine's and alarm's task gets: both about the lab now.
pub const LAB_CRITERIA: [&str; 2] = [
    "Every container that alarmed is running and healthy, from a fresh homelab_status",
    "No new alarm for the same containers since",
];

/// One call of Endora's tools a turn made, as the node recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallReceipt {
    /// Its id: `r<id>` as members cite it.
    pub id: i64,
    /// The turn that made the call.
    pub run: RunId,
    /// The member whose turn it was.
    pub member: super::work::MemberId,
    /// The task the turn was on, if any.
    pub task: Option<TaskId>,
    /// The tool called.
    pub tool: String,
    /// Its arguments, in short.
    pub args: String,
    /// The start of what it gave back.
    pub excerpt: String,
    /// Whether it did what was asked (`false`: refused, or it failed).
    pub ok: bool,
    /// Whether the tool changes something rather than only looks.
    pub act: bool,
    /// Whether what it gave back is Confidential: never shown to a member on a cloud model.
    pub confidential: bool,
    /// Whether the turn's sandbox reported it (ADR 0117) rather than the node witnessing it.
    pub sandbox: bool,
    /// When.
    pub at: Millis,
}

impl CallReceipt {
    /// As it is cited: `r12`.
    #[must_use]
    pub fn said(&self) -> String {
        format!("r{}", self.id)
    }

    /// One line for a reader: `r12 homelab_status (ok): sonarr running…`. For a reader on a
    /// cloud model (`cloud`) a Confidential excerpt is left out, and said to be.
    #[must_use]
    pub fn line(&self, cloud: bool) -> String {
        let how = if self.ok { "ok" } else { "refused" };
        let args = if self.args.is_empty() {
            String::new()
        } else {
            format!(" {}", self.args)
        };
        let excerpt = if self.confidential && cloud {
            "(Confidential: not shown on a cloud model)".to_owned()
        } else {
            self.excerpt.clone()
        };
        let from = if self.sandbox {
            ", sandbox-reported"
        } else {
            ""
        };
        format!(
            "{} {}{args} ({how}{from}): {excerpt}",
            self.said(),
            self.tool
        )
    }
}

/// Text cut to one line of at most `most` characters, with no control characters.
#[must_use]
pub fn short(text: &str, most: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(most)
        .collect()
}

/// What "done" means for a task, in one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    /// What must hold.
    pub text: String,
    /// Whether it is about the lab as it is now: shown only by a reading in the same turn.
    pub live: bool,
}

impl Criterion {
    /// A criterion as a member or the owner wrote it: one line, held to [`CRITERION_MOST`].
    #[must_use]
    pub fn new(text: &str, live: bool) -> Self {
        Self {
            text: short(text, CRITERION_MOST),
            live,
        }
    }

    /// The home lab's standard criteria, both live.
    #[must_use]
    pub fn lab() -> Vec<Self> {
        LAB_CRITERIA.iter().map(|t| Self::new(t, true)).collect()
    }

    /// As they are kept: a JSON list.
    #[must_use]
    pub fn kept(all: &[Self]) -> String {
        if all.is_empty() {
            return String::new();
        }
        let list: Vec<serde_json::Value> = all
            .iter()
            .map(|c| serde_json::json!({ "text": c.text, "live": c.live }))
            .collect();
        serde_json::Value::Array(list).to_string()
    }

    /// Read back from how they are kept; what does not read is left out.
    #[must_use]
    pub fn read(kept: &str) -> Vec<Self> {
        let Ok(serde_json::Value::Array(list)) = serde_json::from_str(kept) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|c| {
                let text = c["text"].as_str()?.trim();
                (!text.is_empty()).then(|| Self::new(text, c["live"].as_bool().unwrap_or(false)))
            })
            .collect()
    }

    /// The owner's criteria from the screen, a line each: `live:` before one marks it live.
    #[must_use]
    pub fn from_lines(lines: &[String]) -> Vec<Self> {
        lines
            .iter()
            .filter_map(|line| {
                let line = line.trim();
                let (text, live) = match line.get(..5) {
                    Some(head) if head.eq_ignore_ascii_case("live:") => (line[5..].trim(), true),
                    _ => (line, false),
                };
                (!text.is_empty()).then(|| Self::new(text, live))
            })
            .take(MOST_CRITERIA)
            .collect()
    }
}

/// Whether a member handing work to `assignee` gave criteria enough: work handed to another
/// member says what done means; the owner's, and a member's own, need not.
///
/// # Errors
/// [`Refusal::NoCriteria`] when work handed on has none, [`Refusal::TooLong`] past
/// [`MOST_CRITERIA`].
pub fn may_hand_on(criteria: &[Criterion], to_another: bool) -> Result<(), Refusal> {
    if criteria.len() > MOST_CRITERIA {
        return Err(Refusal::TooLong(MOST_CRITERIA));
    }
    if to_another && criteria.is_empty() {
        return Err(Refusal::NoCriteria);
    }
    Ok(())
}

/// A piece of evidence a member cites from HQ's record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ref {
    /// A receipt, `r12`.
    Receipt(i64),
    /// A task, `task 12` or `#12`.
    Task(TaskId),
    /// An event in the feed, `event 340` or `e340`: an alarm, a post, a task's move.
    Event(i64),
}

impl Ref {
    /// As members cite it: `r12`, `task 12`, `event 340`.
    #[must_use]
    pub fn said(self) -> String {
        match self {
            Self::Receipt(id) => format!("r{id}"),
            Self::Task(t) => format!("task {t}"),
            Self::Event(e) => format!("event {e}"),
        }
    }

    /// A reference as a member wrote it, if it reads as one: `r12`, `receipt 12`, `#12`,
    /// `task 12`, `e340`, `event 340`, case aside.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        let number = |rest: &str| rest.trim().parse::<i64>().ok().filter(|n| *n > 0);
        for (prefix, make) in [
            ("receipt", Self::Receipt as fn(i64) -> Self),
            ("event", Self::Event),
            ("task", |n| Self::Task(TaskId(n))),
            ("r", Self::Receipt),
            ("e", Self::Event),
            ("#", |n| Self::Task(TaskId(n))),
        ] {
            if let Some(rest) = text.strip_prefix(prefix) {
                return number(rest).map(make);
            }
        }
        None
    }

    /// As it is kept, a list of them: `r12,task 5`.
    #[must_use]
    pub fn kept(all: &[Self]) -> String {
        all.iter().map(|r| r.said()).collect::<Vec<_>>().join(",")
    }

    /// Read back from how a list is kept.
    #[must_use]
    pub fn read(kept: &str) -> Vec<Self> {
        kept.split(',').filter_map(Self::parse).collect()
    }
}

/// What a reference was found to be in HQ's record, as the engine looked it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// Not in the record.
    Missing,
    /// A receipt: from which turn, whether the call did what was asked, and whether it was made
    /// on this work.
    Receipt {
        /// The turn that made the call.
        run: RunId,
        /// Whether it did what was asked.
        ok: bool,
        /// Whether it was made on this work.
        in_work: bool,
        /// Whether the node witnessed it, rather than the turn's sandbox reporting it.
        witnessed: bool,
    },
    /// A task or an event: whether it is about this work.
    Other {
        /// Whether it is about this work.
        in_work: bool,
    },
}

/// How a claim is judged: whose readings count as fresh, and whether the one claiming must
/// have taken readings of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged<'a> {
    /// The turns whose readings are fresh: the claiming turn's own, and for an acceptance the
    /// tester's passing turn.
    pub fresh: &'a [RunId],
    /// The claiming turn: the tester's pass needs a reading of its own.
    pub own_reading: Option<RunId>,
}

/// The one evidence rule (ADR 0114). `cited[i]` is criterion `i + 1`'s evidence; with no
/// criteria, the claim still needs one reference (the work itself is its criterion). Every
/// reference must be in HQ's record, about this work, and not a refused call; a live criterion
/// needs a reading from a fresh turn that the node witnessed (a sandbox's own report of its
/// tools is no reading of the lab, ADR 0117); and a tester's pass a reading of her own.
///
/// # Errors
/// The [`Refusal`] naming what is missing, and for which criterion.
pub fn judge(
    criteria: &[Criterion],
    cited: &[Vec<(Ref, Found)>],
    how: &Judged<'_>,
) -> Result<(), Refusal> {
    for (r, found) in cited.iter().flatten() {
        match found {
            Found::Missing => return Err(Refusal::NoSuchEvidence(r.said())),
            Found::Receipt { in_work: false, .. } | Found::Other { in_work: false } => {
                return Err(Refusal::NotThisWork(r.said()));
            }
            Found::Receipt { ok: false, .. } => return Err(Refusal::RefusedCall(r.said())),
            _ => {}
        }
    }
    let fresh = |group: &Vec<(Ref, Found)>| {
        group.iter().any(
            |(_, f)| matches!(f, Found::Receipt { run, ok: true, witnessed: true, .. } if how.fresh.contains(run)),
        )
    };
    if criteria.is_empty() && cited.iter().all(Vec::is_empty) {
        return Err(Refusal::NoEvidenceFor {
            criterion: 0,
            text: String::new(),
        });
    }
    for (i, criterion) in criteria.iter().enumerate() {
        let empty = Vec::new();
        let group = cited.get(i).unwrap_or(&empty);
        if group.is_empty() {
            return Err(Refusal::NoEvidenceFor {
                criterion: i + 1,
                text: criterion.text.clone(),
            });
        }
        if criterion.live && !fresh(group) {
            return Err(Refusal::NotFresh {
                criterion: i + 1,
                text: criterion.text.clone(),
            });
        }
    }
    if let Some(turn) = how.own_reading {
        let own = cited
            .iter()
            .flatten()
            .any(|(_, f)| matches!(f, Found::Receipt { run, ok: true, .. } if *run == turn));
        if !own {
            return Err(Refusal::NoOwnReading);
        }
    }
    Ok(())
}

/// What a turn came to, worked out from what happened in it (ADR 0114).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// Something changed: a task moved, a project changed, an act succeeded.
    Advanced,
    /// It left its task blocked, or put a concrete blocker to someone (a question, an approval).
    Blocked,
    /// It took readings.
    Checked,
    /// It posted, but took no reading and changed nothing.
    TalkOnly,
    /// Nothing at all.
    Empty,
}

impl Label {
    /// Stable name, as stored and shown.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Advanced => "advanced",
            Self::Blocked => "blocked",
            Self::Checked => "checked",
            Self::TalkOnly => "talk only",
            Self::Empty => "empty",
        }
    }

    /// The label with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Advanced,
            Self::Blocked,
            Self::Checked,
            Self::TalkOnly,
            Self::Empty,
        ]
        .into_iter()
        .find(|l| l.name() == name)
    }

    /// Whether the turn did something: the empty-turn back-off and the nudges pass it by.
    #[must_use]
    pub const fn productive(self) -> bool {
        !matches!(self, Self::TalkOnly | Self::Empty)
    }
}

/// What happened in a turn, as the record says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TurnFacts {
    /// A task's state moved or a project changed through HQ.
    pub changed: bool,
    /// An act of Endora's succeeded (a receipt that acts and is ok).
    pub acted: bool,
    /// How many readings succeeded (receipts that are ok).
    pub readings: usize,
    /// It posted in a thread.
    pub posted: bool,
    /// It left its task blocked, or asked up the chain, the owner, or for an approval.
    pub blocked: bool,
}

/// A turn's label from its facts, the first that holds: advanced, blocked, checked, talk only,
/// empty.
#[must_use]
pub const fn label(facts: TurnFacts) -> Label {
    if facts.changed || facts.acted {
        return Label::Advanced;
    }
    if facts.blocked {
        return Label::Blocked;
    }
    if facts.readings > 0 {
        return Label::Checked;
    }
    if facts.posted {
        return Label::TalkOnly;
    }
    Label::Empty
}

/// What follows a member's turn on its task that showed nothing, the `count`th such
/// (counting it): a nudge for the first [`MOST_NUDGES`], then the task goes up once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterNothing {
    /// The next turn is told to take a concrete step.
    Nudge,
    /// The task goes to whom the member reports to, with a post saying so.
    HandUp,
    /// It went up already.
    Nothing,
}

/// What follows the `count`th turn that showed nothing.
#[must_use]
pub const fn after_nothing(count: usize) -> AfterNothing {
    if count <= MOST_NUDGES {
        return AfterNothing::Nudge;
    }
    if count == MOST_NUDGES + 1 {
        return AfterNothing::HandUp;
    }
    AfterNothing::Nothing
}

/// Whether work sent back `sent_back` times goes to the owner on its next submission.
#[must_use]
pub const fn to_the_owner(sent_back: usize) -> bool {
    sent_back >= MOST_ROUNDS
}

/// Who checks submitted work: the tester, then the lead; one alone when the other is missing
/// or did the work; the owner when neither can (ADR 0114). A member never checks its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkers<M> {
    /// The tester tests, then the lead accepts.
    Both {
        /// The tester.
        tester: M,
        /// The lead.
        lead: M,
    },
    /// No other lead: the tester's pass closes it.
    TesterAlone(M),
    /// No other tester: the lead accepts alone.
    LeadAlone(M),
    /// Nobody else: the owner reads it.
    Owner,
}

/// Who checks work done by `worker`, of the team's `tester` and the work's `lead`.
#[must_use]
pub fn checkers<M: PartialEq + Copy>(
    worker: Option<M>,
    tester: Option<M>,
    lead: Option<M>,
) -> Checkers<M> {
    let tester = tester.filter(|t| Some(*t) != worker);
    let lead = lead.filter(|l| Some(*l) != worker);
    match (tester, lead) {
        (Some(tester), Some(lead)) => Checkers::Both { tester, lead },
        (Some(tester), None) => Checkers::TesterAlone(tester),
        (None, Some(lead)) => Checkers::LeadAlone(lead),
        (None, None) => Checkers::Owner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: RunId = RunId(9);
    const BEFORE: RunId = RunId(3);

    fn reading(run: RunId) -> (Ref, Found) {
        (
            Ref::Receipt(run.0 * 10),
            Found::Receipt {
                run,
                ok: true,
                in_work: true,
                witnessed: true,
            },
        )
    }

    fn task_ref() -> (Ref, Found) {
        (Ref::Task(TaskId(4)), Found::Other { in_work: true })
    }

    fn now() -> Judged<'static> {
        Judged {
            fresh: &[NOW],
            own_reading: None,
        }
    }

    #[test]
    fn evidence_is_cited_as_receipts_tasks_and_events_and_read_back() {
        assert_eq!(Ref::parse("r12"), Some(Ref::Receipt(12)));
        assert_eq!(Ref::parse(" Receipt 12 "), Some(Ref::Receipt(12)));
        assert_eq!(Ref::parse("#7"), Some(Ref::Task(TaskId(7))));
        assert_eq!(Ref::parse("task 7"), Some(Ref::Task(TaskId(7))));
        assert_eq!(Ref::parse("E340"), Some(Ref::Event(340)));
        assert_eq!(Ref::parse("event 340"), Some(Ref::Event(340)));
        for bad in ["", "r", "rx", "r-1", "r0", "the log", "12"] {
            assert_eq!(Ref::parse(bad), None, "{bad:?}");
        }
        let all = [Ref::Receipt(1), Ref::Task(TaskId(2)), Ref::Event(3)];
        assert_eq!(Ref::kept(&all), "r1,task 2,event 3");
        assert_eq!(Ref::read(&Ref::kept(&all)), all);
    }

    #[test]
    fn criteria_are_kept_and_the_owners_lines_mark_live_ones() {
        let all = [
            Criterion::new("sonarr is up", true),
            Criterion::new("a note", false),
        ];
        assert_eq!(Criterion::read(&Criterion::kept(&all)), all);
        assert_eq!(Criterion::read(""), []);
        let lines = ["live: radarr answers", "  ", "the queue is empty"].map(str::to_owned);
        assert_eq!(
            Criterion::from_lines(&lines),
            [
                Criterion::new("radarr answers", true),
                Criterion::new("the queue is empty", false)
            ]
        );
        assert!(Criterion::lab().iter().all(|c| c.live));
        assert_eq!(Criterion::lab().len(), 2);
    }

    #[test]
    fn work_handed_to_another_member_says_what_done_means() {
        assert_eq!(may_hand_on(&[], true), Err(Refusal::NoCriteria));
        assert!(may_hand_on(&[], false).is_ok(), "the owner's, or one's own");
        assert!(may_hand_on(&[Criterion::new("x", false)], true).is_ok());
        let many = vec![Criterion::new("x", false); MOST_CRITERIA + 1];
        assert_eq!(
            may_hand_on(&many, true),
            Err(Refusal::TooLong(MOST_CRITERIA))
        );
    }

    #[test]
    fn done_is_refused_naming_the_criterion_that_has_no_evidence() {
        let criteria = [
            Criterion::new("the cause is found", false),
            Criterion::new("the fix is written up", false),
        ];
        let refused = judge(&criteria, &[vec![task_ref()]], &now());
        assert_eq!(
            refused,
            Err(Refusal::NoEvidenceFor {
                criterion: 2,
                text: "the fix is written up".to_owned()
            })
        );
        assert!(
            refused
                .unwrap_err()
                .to_string()
                .contains("criterion 2 (the fix is written up) has no evidence"),
        );
        assert!(judge(&criteria, &[vec![task_ref()], vec![task_ref()]], &now()).is_ok());
        assert_eq!(
            judge(&[], &[], &now()),
            Err(Refusal::NoEvidenceFor {
                criterion: 0,
                text: String::new()
            }),
            "with no criteria the work itself needs evidence"
        );
        assert!(judge(&[], &[vec![task_ref()]], &now()).is_ok());
    }

    #[test]
    fn a_live_criterion_needs_a_reading_from_this_turn_not_an_older_one() {
        let live = [Criterion::new("sonarr is running", true)];
        assert_eq!(
            judge(&live, &[vec![reading(BEFORE)]], &now()),
            Err(Refusal::NotFresh {
                criterion: 1,
                text: "sonarr is running".to_owned()
            })
        );
        assert_eq!(
            judge(&live, &[vec![task_ref()]], &now()),
            Err(Refusal::NotFresh {
                criterion: 1,
                text: "sonarr is running".to_owned()
            }),
            "another member's word is no reading"
        );
        assert!(judge(&live, &[vec![reading(BEFORE), reading(NOW)]], &now()).is_ok());
    }

    #[test]
    fn evidence_must_be_in_the_record_about_this_work_and_not_a_refused_call() {
        let c = [Criterion::new("x", false)];
        let missing = [vec![(Ref::Receipt(99), Found::Missing)]];
        assert_eq!(
            judge(&c, &missing, &now()),
            Err(Refusal::NoSuchEvidence("r99".to_owned()))
        );
        let elsewhere = [vec![(Ref::Event(5), Found::Other { in_work: false })]];
        assert_eq!(
            judge(&c, &elsewhere, &now()),
            Err(Refusal::NotThisWork("event 5".to_owned()))
        );
        let refused = [vec![(
            Ref::Receipt(4),
            Found::Receipt {
                run: NOW,
                ok: false,
                in_work: true,
                witnessed: true,
            },
        )]];
        assert_eq!(
            judge(&c, &refused, &now()),
            Err(Refusal::RefusedCall("r4".to_owned()))
        );
    }

    #[test]
    fn the_testers_pass_needs_a_reading_of_her_own() {
        let c = [Criterion::new("x", false)];
        let hers = Judged {
            fresh: &[NOW],
            own_reading: Some(NOW),
        };
        assert_eq!(
            judge(&c, &[vec![reading(BEFORE)]], &hers),
            Err(Refusal::NoOwnReading)
        );
        assert!(judge(&c, &[vec![reading(BEFORE), reading(NOW)]], &hers).is_ok());
    }

    #[test]
    fn a_turns_label_comes_from_what_happened_in_it() {
        let f = TurnFacts::default();
        assert_eq!(label(f), Label::Empty);
        assert_eq!(label(TurnFacts { posted: true, ..f }), Label::TalkOnly);
        assert_eq!(
            label(TurnFacts {
                posted: true,
                readings: 2,
                ..f
            }),
            Label::Checked
        );
        assert_eq!(
            label(TurnFacts {
                readings: 1,
                blocked: true,
                ..f
            }),
            Label::Blocked
        );
        assert_eq!(
            label(TurnFacts {
                acted: true,
                blocked: true,
                ..f
            }),
            Label::Advanced
        );
        assert_eq!(label(TurnFacts { changed: true, ..f }), Label::Advanced);
        for l in [
            Label::Advanced,
            Label::Blocked,
            Label::Checked,
            Label::TalkOnly,
            Label::Empty,
        ] {
            assert_eq!(Label::from_name(l.name()), Some(l));
        }
        assert!(Label::Blocked.productive() && Label::Checked.productive());
        assert!(!Label::TalkOnly.productive() && !Label::Empty.productive());
    }

    #[test]
    fn two_nudges_then_the_task_goes_up_once() {
        assert_eq!(after_nothing(1), AfterNothing::Nudge);
        assert_eq!(after_nothing(2), AfterNothing::Nudge);
        assert_eq!(after_nothing(3), AfterNothing::HandUp);
        assert_eq!(after_nothing(4), AfterNothing::Nothing);
        assert!(!to_the_owner(2) && to_the_owner(3));
    }

    #[test]
    fn nobody_checks_its_own_work_and_the_fallbacks_say_who_does() {
        let (dev, dee, ada) = (Some(1), Some(2), Some(3));
        assert_eq!(
            checkers(dev, dee, ada),
            Checkers::Both { tester: 2, lead: 3 }
        );
        assert_eq!(checkers(dev, None, ada), Checkers::LeadAlone(3));
        assert_eq!(
            checkers(dee, dee, ada),
            Checkers::LeadAlone(3),
            "the tester's own"
        );
        assert_eq!(
            checkers(ada, dee, ada),
            Checkers::TesterAlone(2),
            "the lead's own"
        );
        assert_eq!(checkers(dev, None, None), Checkers::Owner);
    }

    #[test]
    fn a_receipts_line_hides_a_confidential_excerpt_from_a_cloud_reader() {
        let r = CallReceipt {
            id: 12,
            run: NOW,
            member: super::super::work::MemberId(1),
            task: Some(TaskId(4)),
            tool: "container_logs".to_owned(),
            args: "container: sonarr".to_owned(),
            excerpt: "GET /api 500".to_owned(),
            ok: true,
            act: false,
            confidential: true,
            sandbox: false,
            at: 0,
        };
        assert_eq!(
            r.line(false),
            "r12 container_logs container: sonarr (ok): GET /api 500"
        );
        assert!(!r.line(true).contains("GET /api"));
        let reported = CallReceipt {
            tool: "Bash".to_owned(),
            args: "git status".to_owned(),
            excerpt: "clean".to_owned(),
            confidential: false,
            sandbox: true,
            ..r
        };
        assert_eq!(
            reported.line(true),
            "r12 Bash git status (ok, sandbox-reported): clean"
        );
    }

    #[test]
    fn a_sandbox_report_may_be_cited_but_a_live_criterion_needs_the_nodes_own_reading() {
        let reported = |run: RunId| {
            (
                Ref::Receipt(run.0 * 10 + 1),
                Found::Receipt {
                    run,
                    ok: true,
                    in_work: true,
                    witnessed: false,
                },
            )
        };
        let plain = [Criterion::new("the role's tasks are tidied", false)];
        assert!(
            judge(&plain, &[vec![reported(NOW)]], &now()).is_ok(),
            "cited for what the sandbox did"
        );
        let live = [Criterion::new("sonarr is running", true)];
        assert_eq!(
            judge(&live, &[vec![reported(NOW)]], &now()),
            Err(Refusal::NotFresh {
                criterion: 1,
                text: "sonarr is running".to_owned()
            }),
            "a sandbox's word is no reading of the lab"
        );
        assert!(judge(&live, &[vec![reported(NOW), reading(NOW)]], &now()).is_ok());
    }
}
