//! The team's history (ADR 0129): what a turn said, kept for a while, and what is kept for good.
//!
//! - A **transcript** is one turn's whole conversation as plain text, scrubbed of secrets by the
//!   node before it reaches HQ and capped here. HQ keeps it for [`KEPT_DAYS`] days and at most
//!   the last [`KEPT_PER`] for each member and task; after that only the handover notes and the
//!   posts (a turn's last words among them) remain.
//! - A transcript carries the **tier** of what its turn read: a turn that saw Confidential data
//!   (ADR 0105, 0108) is marked, so that a search on behalf of a member on a cloud model can
//!   leave it out.
//!
//! Pure: no clock, no store, no network.

use core::fmt;

use endora_kernel::tier::Tier;

use super::work::{GoalId, MemberId, Millis, RunId, TaskId};

/// Source identity recorded by an authenticated application write, never by prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOrigin {
    /// No reliable forward-recorded source; readers must fail closed.
    Unknown,
    /// An actual owner write, selected by application authority.
    Owner,
    /// The authenticated run that supplied these bytes.
    Run(RunId),
}

/// Independently authored inputs: changing criteria cannot relabel a task body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRef {
    /// Goal title/reason and parent link.
    Goal(GoalId),
    /// Task title/description and goal/parent links.
    TaskBody(TaskId),
    /// A task's current acceptance criteria.
    TaskCriteria(TaskId),
    /// One immutable-positioned task post.
    Post(i64),
}

/// A day, in milliseconds.
pub const DAY_MS: Millis = 24 * 60 * 60 * 1000;

/// The most bytes of a turn's conversation HQ keeps: head and tail, the cut marked.
pub const TRANSCRIPT_MOST: usize = 1024 * 1024;

/// How long a transcript is kept, in days.
pub const KEPT_DAYS: i64 = 30;

/// How many transcripts are kept for one member on one task: the latest.
pub const KEPT_PER: usize = 5;

/// What a cut transcript says where the middle was.
fn cut_mark(removed: usize) -> String {
    format!("\n[... cut by HQ: {removed} bytes removed from the middle of this conversation ...]\n")
}

/// `text` held to `most` bytes, keeping its head and its tail and marking where the middle was
/// cut. Returns the text and whether it was cut. The result never exceeds `most` bytes and is
/// cut only between characters.
#[must_use]
pub fn cap(text: &str, most: usize) -> (String, bool) {
    if text.len() <= most {
        return (text.to_owned(), false);
    }
    // The mark's own length depends on how much it says it removed; its digits are at most as
    // many as the text's length has.
    let mark_room = cut_mark(text.len()).len();
    let room = most.saturating_sub(mark_room);
    let head_len = room / 2;
    let tail_len = room - head_len;
    let mut head_end = head_len.min(text.len());
    while !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len().saturating_sub(tail_len);
    while !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let removed = tail_start.saturating_sub(head_end);
    let mut out = String::with_capacity(most);
    out.push_str(&text[..head_end]);
    out.push_str(&cut_mark(removed));
    out.push_str(&text[tail_start..]);
    (out, true)
}

/// The tier of what a turn read, on the house's own scale (ADR 0105): the highest tier of
/// anything it read. A turn that read a result the node marked Confidential is Confidential;
/// one that did not is at most Internal, the tier of HQ's own workings.
#[must_use]
pub const fn tier_of(read_confidential: bool) -> Tier {
    if read_confidential {
        Tier::Confidential
    } else {
        Tier::Internal
    }
}

/// A tier's stable name, for storage and the screen.
#[must_use]
pub const fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Public => "public",
        Tier::Internal => "internal",
        Tier::Confidential => "confidential",
        Tier::Restricted => "restricted",
    }
}

/// The tier with this [`tier_name`]; anything else is read as the strictest, so a stored value
/// nobody understands is never shown to a cloud model.
#[must_use]
pub fn tier_from_name(name: &str) -> Tier {
    [Tier::Public, Tier::Internal, Tier::Confidential]
        .into_iter()
        .find(|t| tier_name(*t) == name)
        .unwrap_or(Tier::Restricted)
}

/// Whether what a turn of this tier said may be given to a member thinking on a cloud model:
/// nothing from a turn that read Confidential data or above is (ADR 0129).
#[must_use]
pub fn may_reach_a_cloud_member(tier: Tier) -> bool {
    tier < Tier::Confidential
}

/// One turn's conversation, as HQ keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// The turn.
    pub run: RunId,
    /// Whose turn.
    pub member: MemberId,
    /// The task it was on, if any.
    pub task: Option<TaskId>,
    /// What ran it: `worker` or `claude-code`.
    pub runtime: String,
    /// What its member thought with, as it was then (`local`, or a Claude model's name).
    pub brain: String,
    /// What it read.
    pub tier: Tier,
    /// About how many tokens it comes to.
    pub est_tokens: i64,
    /// Whether the middle was cut to fit [`TRANSCRIPT_MOST`].
    pub cut: bool,
    /// The conversation, as plain text.
    pub body: String,
    /// When it was kept.
    pub at: Millis,
    /// Until when.
    pub kept_until: Millis,
}

impl Transcript {
    /// As the owner's replay reads it.
    #[must_use]
    pub fn shown(&self) -> serde_json::Value {
        serde_json::json!({
            "runtime": self.runtime, "brain": self.brain, "tier": tier_name(self.tier),
            "est_tokens": self.est_tokens, "cut": self.cut, "kept_until": self.kept_until,
            "text": self.body,
        })
    }
}

/// What the retention rule needs to know of a transcript: no body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    /// The turn.
    pub run: RunId,
    /// Whose turn.
    pub member: MemberId,
    /// The task, if any.
    pub task: Option<TaskId>,
    /// When it was kept.
    pub at: Millis,
    /// Until when.
    pub kept_until: Millis,
}

/// Which transcripts to let go of at `now`: those past their time, and, of what remains for one
/// member on one task, all but the latest [`KEPT_PER`].
#[must_use]
pub fn to_prune(kept: &[Kept], now: Millis) -> Vec<RunId> {
    let mut gone: Vec<RunId> = kept
        .iter()
        .filter(|k| k.kept_until <= now)
        .map(|k| k.run)
        .collect();
    let mut live: Vec<&Kept> = kept.iter().filter(|k| k.kept_until > now).collect();
    // Newest first, so that the first KEPT_PER of each group stay.
    live.sort_by_key(|k| std::cmp::Reverse((k.at, k.run)));
    let mut seen: std::collections::HashMap<(MemberId, Option<TaskId>), usize> =
        std::collections::HashMap::new();
    for k in live {
        let n = seen.entry((k.member, k.task)).or_insert(0);
        *n += 1;
        if *n > KEPT_PER {
            gone.push(k.run);
        }
    }
    gone.sort();
    gone
}

/// What the history refuses, said so a member can do something about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryRefusal {
    /// The turn's conversation is already kept: a turn has one.
    TranscriptKept,
    /// A search with no words to search for.
    NothingToSearchFor,
    /// A name that is no member of the team.
    NoSuchMember(String),
    /// A consult of oneself.
    NotYourself,
    /// The CEO consulting anyone but a lead.
    CeoConsultsLeads,
    /// A third consult in one turn.
    TooManyThisTurn,
    /// A seventh consult in one day.
    TooManyToday,
    /// A consult without a question.
    NeedsQuestion,
    /// A consult from a turn about no task, naming none.
    NeedsTask,
    /// `answer_consult` outside a consulted turn, or twice.
    NoConsultForYou,
    /// An answer with nothing in it.
    NeedsAnswer,
}

impl fmt::Display for HistoryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TranscriptKept => f.write_str("this turn's conversation is already kept"),
            Self::NothingToSearchFor => f.write_str(
                "ask_record searches for the distinctive words of your question: give a question \
                 with a few (a name, a thing, what was decided)",
            ),
            Self::NoSuchMember(name) => write!(
                f,
                "nobody on the team is called `{name}`: read world:team for who is at work"
            ),
            Self::NotYourself => f.write_str(
                "you cannot consult yourself: ask_record searches what you and the others said",
            ),
            Self::CeoConsultsLeads => f.write_str(
                "the CEO consults leads only (ADR 0120 keeps the CEO out of the weeds): consult \
                 a lead, or direct one",
            ),
            Self::TooManyThisTurn => f.write_str(
                "you have already consulted twice in this turn, the most a turn may: use \
                 ask_record, which searches the record and wakes no one, or wait for their answers",
            ),
            Self::TooManyToday => f.write_str(
                "you have consulted six times today, the most a member may in a day: use \
                 ask_record, which searches the record and costs nothing",
            ),
            Self::NeedsQuestion => {
                f.write_str("a consult asks something: give `question`, in a sentence or two")
            }
            Self::NeedsTask => f.write_str(
                "this turn is about no task, and the answer goes in a task's thread: name one \
                 with `task`",
            ),
            Self::NoConsultForYou => {
                f.write_str("no consult is waiting for an answer from you in this turn")
            }
            Self::NeedsAnswer => f.write_str("an answer says something: give `answer`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_within_the_cap_is_kept_whole() {
        let (kept, cut) = cap("a short talk", 100);
        assert_eq!((kept.as_str(), cut), ("a short talk", false));
    }

    #[test]
    fn text_over_the_cap_keeps_its_head_and_tail_and_says_where_it_was_cut() {
        let text = format!("HEAD{}TAIL", "x".repeat(10_000));
        let (kept, cut) = cap(&text, 1_000);
        assert!(cut);
        assert!(kept.len() <= 1_000, "{}", kept.len());
        assert!(kept.starts_with("HEAD") && kept.ends_with("TAIL"));
        assert!(kept.contains("cut by HQ"), "the cut is marked: {kept}");
        let removed: usize = kept
            .split("cut by HQ: ")
            .nth(1)
            .and_then(|r| r.split(' ').next())
            .and_then(|n| n.parse().ok())
            .expect("the mark says how much was removed");
        assert_eq!(removed + kept.len() - cut_mark(removed).len(), text.len());
    }

    #[test]
    fn a_cap_cuts_between_characters_never_inside_one() {
        let text = "é".repeat(5_000);
        let (kept, cut) = cap(&text, 999);
        assert!(cut && kept.len() <= 999);
        assert!(kept.starts_with('é') && kept.ends_with('é'));
    }

    #[test]
    fn a_tier_is_read_back_and_an_unknown_one_is_the_stricter() {
        for tier in [Tier::Public, Tier::Internal, Tier::Confidential] {
            assert_eq!(tier_from_name(tier_name(tier)), tier);
        }
        assert_eq!(tier_from_name("something else"), Tier::Restricted);
        assert_eq!(tier_of(true), Tier::Confidential);
        assert_eq!(tier_of(false), Tier::Internal);
        assert!(may_reach_a_cloud_member(Tier::Internal));
        assert!(!may_reach_a_cloud_member(Tier::Confidential));
        assert!(!may_reach_a_cloud_member(Tier::Restricted));
    }

    fn kept(run: i64, member: i64, task: Option<i64>, day: i64) -> Kept {
        Kept {
            run: RunId(run),
            member: MemberId(member),
            task: task.map(TaskId),
            at: day * DAY_MS,
            kept_until: (day + KEPT_DAYS) * DAY_MS,
        }
    }

    #[test]
    fn what_is_past_its_thirty_days_is_let_go() {
        let all = [kept(1, 1, Some(1), 0), kept(2, 1, Some(1), 20)];
        assert_eq!(to_prune(&all, 31 * DAY_MS), vec![RunId(1)]);
        assert!(to_prune(&all, 29 * DAY_MS).is_empty());
    }

    #[test]
    fn only_the_last_five_of_a_member_on_a_task_stay() {
        let mut all: Vec<Kept> = (1..=7).map(|n| kept(n, 1, Some(9), n)).collect();
        // Another member's, and the same member's on another task and on none, are counted apart.
        all.push(kept(10, 2, Some(9), 1));
        all.push(kept(11, 1, Some(8), 1));
        all.push(kept(12, 1, None, 1));
        assert_eq!(to_prune(&all, 8 * DAY_MS), vec![RunId(1), RunId(2)]);
    }
}
