//! Situations and the briefing's budget (ADR 0122): a turn starts from one of six situations,
//! decided by code from the record, and what it is told of the team's world is composed for that
//! situation within a budget. What does not fit is named at the end of the briefing, with how
//! to read it: nothing is ever cut without a word, and what the situation needs most is never
//! cut at all.

use super::brain::Brain;
use super::work::{Millis, Role};

/// A member's last turn on its task this long ago or more starts it after a gap.
pub const GAP_ON_TASK_MS: Millis = 24 * 60 * 60 * 1000;
/// A member's last turn of any kind this long ago or more starts it after a gap.
pub const GAP_ANY_MS: Millis = 7 * 24 * 60 * 60 * 1000;

/// What a turn starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Situation {
    /// The member's first turn ever, or its first in this project.
    Fresh,
    /// Its last turn on this work was recent.
    Continuing,
    /// Its last turn on this work was a day ago or more, or its last turn at all a week ago.
    AfterGap,
    /// The work was handed to it, or it is new to work another member took turns on.
    Handover,
    /// Its first turn after a furlough (ADR 0118).
    Recalled,
    /// A runtime that carries its own earlier conversation (later: not yet built).
    Resumed,
}

/// Every situation, in the order they are decided.
pub const ALL: [Situation; 6] = [
    Situation::Recalled,
    Situation::Handover,
    Situation::Fresh,
    Situation::AfterGap,
    Situation::Continuing,
    Situation::Resumed,
];

impl Situation {
    /// Its stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Continuing => "continuing",
            Self::AfterGap => "after_gap",
            Self::Handover => "handover",
            Self::Recalled => "recalled",
            Self::Resumed => "resumed",
        }
    }

    /// The situation with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        ALL.into_iter().find(|s| s.name() == name.trim())
    }

    /// Whether this situation costs more to brief: the rare ones that need the most.
    #[must_use]
    pub const fn dear(self) -> bool {
        matches!(self, Self::AfterGap | Self::Handover | Self::Recalled)
    }
}

/// What the record says about a member and the turn it is about to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Facts {
    /// It was recalled from a furlough and has taken no turn since.
    pub recalled: bool,
    /// A handover note to it on this task is newer than its last turn on it.
    pub handed_a_note: bool,
    /// Another member took the task's last turn, and it has taken none on the task before.
    pub new_to_the_work: bool,
    /// It has never taken a turn.
    pub first_turn: bool,
    /// The task is in a project it has never taken a turn in.
    pub first_in_project: bool,
    /// When its last turn on this task started, if it took one.
    pub last_on_task: Option<Millis>,
    /// When its last turn of any kind started, if it took one.
    pub last_turn: Option<Millis>,
    /// Now.
    pub now: Millis,
}

/// The situation a turn starts from: recalled, then handed over, then fresh, then after a gap,
/// and otherwise continuing.
#[must_use]
pub fn decide(facts: &Facts) -> Situation {
    if facts.recalled {
        return Situation::Recalled;
    }
    if facts.handed_a_note || facts.new_to_the_work {
        return Situation::Handover;
    }
    if facts.first_turn || facts.first_in_project {
        return Situation::Fresh;
    }
    let long = |at: Option<Millis>, gap: Millis| at.is_some_and(|at| facts.now - at >= gap);
    if long(facts.last_on_task, GAP_ON_TASK_MS) || long(facts.last_turn, GAP_ANY_MS) {
        return Situation::AfterGap;
    }
    Situation::Continuing
}

/// The NAS's model's budget, whatever the role: its context is small.
pub const LOCAL_BUDGET: usize = 1_800;
/// The Mac's budget, whatever the role: more than the NAS's, and short of what a lead has on
/// Claude.
pub const MAC_BUDGET: usize = 2_800;
/// What the dear situations add to any budget.
pub const DEAR_EXTRA: usize = 1_200;

/// How many tokens of the world a member's briefing may carry: the CEO 2,500 (its briefings
/// are held short), a lead 4,000 (it stewards its area), anyone else 3,000; on the NAS's model
/// never more than 1,800, and on the Mac never more than 2,800 (ADR 0130, amended); and 1,200
/// more after a gap, on a handover or a recall.
#[must_use]
pub const fn budget(role: Role, brain: Brain, situation: Situation) -> usize {
    let mut most = match role {
        Role::Ceo => 2_500,
        Role::Lead => 4_000,
        _ => 3_000,
    };
    let cap = match brain {
        Brain::Local => LOCAL_BUDGET,
        Brain::Mac => MAC_BUDGET,
        Brain::Sonnet | Brain::Haiku | Brain::Opus | Brain::DeepSeek => usize::MAX,
    };
    if most > cap {
        most = cap;
    }
    if situation.dear() {
        most += DEAR_EXTRA;
    }
    most
}

/// How much a briefing may leave out of a piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weight {
    /// Never left out: what the situation is about.
    Core,
    /// Left out only when the optional pieces are gone and it still does not fit.
    Recommended,
    /// Left out first.
    Optional,
}

/// One piece of a briefing: what it says, how it is named when it is left out, and its weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// How it is named when it is left out: an address, or `your notebook`.
    pub name: String,
    /// What it says, ending in a line break.
    pub text: String,
    /// How much may be left out.
    pub weight: Weight,
}

impl Piece {
    /// A piece.
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<String>, weight: Weight) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
            weight,
        }
    }
}

/// What a briefing carries of the world, and what it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    /// The pieces kept, in their order.
    pub text: String,
    /// The line that names what was left out and how to read it; empty when nothing was.
    pub omitted: String,
    /// The names of the pieces kept.
    pub briefed: Vec<String>,
    /// The names of the pieces left out.
    pub dropped: Vec<String>,
    /// About how many tokens the kept pieces and the line take.
    pub tokens: usize,
    /// The budget it was composed in.
    pub budget: usize,
    /// Whether what is never left out ran past the budget on its own.
    pub over_budget: bool,
}

/// What a turn's run records of its briefing: the situation it started from, what of the
/// world it was told and what was left out, and its size against its budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Briefed {
    /// The turn.
    pub run: super::work::RunId,
    /// The situation it started from.
    pub situation: Situation,
    /// The names of the pieces it was told.
    pub briefed: Vec<String>,
    /// The names of the pieces left out.
    pub dropped: Vec<String>,
    /// About how many tokens of the world it carried.
    pub tokens: usize,
    /// Its budget.
    pub budget: usize,
    /// Whether what is never left out ran past the budget on its own.
    pub over_budget: bool,
    /// When it was briefed.
    pub at: Millis,
}

impl Briefed {
    /// As the replay and the snapshot show it.
    #[must_use]
    pub fn shown(&self) -> serde_json::Value {
        serde_json::json!({
            "situation": self.situation.name(), "briefed": self.briefed,
            "dropped": self.dropped, "tokens": self.tokens, "budget": self.budget,
            "over_budget": self.over_budget,
        })
    }
}

/// About how many tokens `text` takes: a token to four characters.
#[must_use]
pub fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// The line that names what was left out.
fn omitted_line(dropped: &[String]) -> String {
    if dropped.is_empty() {
        return String::new();
    }
    format!(
        "Not included this turn, to keep to your briefing's budget: {}. Read any of them with \
         read_world (your notes with read_notebook).\n",
        dropped.join(", ")
    )
}

/// The pieces composed within `budget` tokens: the optional left out first, then the
/// recommended, the biggest first each time; never one of the core. Each one left out is named.
#[must_use]
pub fn compose(budget: usize, pieces: Vec<Piece>) -> Composed {
    let pieces: Vec<Piece> = pieces.into_iter().filter(|p| !p.text.is_empty()).collect();
    let mut kept = vec![true; pieces.len()];
    let size = |p: &Piece| tokens(&p.text);
    let dropped_names = |kept: &[bool]| -> Vec<String> {
        pieces
            .iter()
            .zip(kept)
            .filter(|(_, k)| !**k)
            .map(|(p, _)| p.name.clone())
            .collect()
    };
    let total = |kept: &[bool]| -> usize {
        let in_it: usize = pieces
            .iter()
            .zip(kept)
            .filter(|(_, k)| **k)
            .map(|(p, _)| size(p))
            .sum();
        in_it + tokens(&omitted_line(&dropped_names(kept)))
    };
    let mut over_budget = false;
    while total(&kept) > budget {
        let next = [Weight::Optional, Weight::Recommended]
            .into_iter()
            .find_map(|weight| {
                pieces
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| kept[*i] && p.weight == weight)
                    .max_by_key(|(i, p)| (size(p), usize::MAX - i))
                    .map(|(i, _)| i)
            });
        let Some(next) = next else {
            over_budget = true;
            break;
        };
        kept[next] = false;
    }
    let dropped = dropped_names(&kept);
    let omitted = omitted_line(&dropped);
    let mut text = String::new();
    let mut briefed = Vec::new();
    for (piece, _) in pieces.iter().zip(&kept).filter(|(_, k)| **k) {
        text.push_str(&piece.text);
        if !briefed.contains(&piece.name) {
            briefed.push(piece.name.clone());
        }
    }
    Composed {
        tokens: tokens(&text) + tokens(&omitted),
        text,
        omitted,
        briefed,
        dropped,
        budget,
        over_budget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Millis = 60 * 60 * 1000;

    #[test]
    fn the_situation_is_decided_from_the_record_in_one_order() {
        let now = 1_000 * HOUR;
        let base = Facts {
            now,
            last_on_task: Some(now - HOUR),
            last_turn: Some(now - HOUR),
            ..Facts::default()
        };
        assert_eq!(decide(&base), Situation::Continuing);
        assert_eq!(
            decide(&Facts {
                last_on_task: Some(now - 24 * HOUR),
                ..base
            }),
            Situation::AfterGap
        );
        assert_eq!(
            decide(&Facts {
                last_on_task: None,
                last_turn: Some(now - 7 * 24 * HOUR),
                ..base
            }),
            Situation::AfterGap,
            "a week away from everything"
        );
        assert_eq!(
            decide(&Facts {
                last_on_task: None,
                ..base
            }),
            Situation::Continuing,
            "new work, but the member was at work an hour ago"
        );
        assert_eq!(
            decide(&Facts {
                first_turn: true,
                last_turn: None,
                last_on_task: None,
                ..base
            }),
            Situation::Fresh
        );
        assert_eq!(
            decide(&Facts {
                first_in_project: true,
                ..base
            }),
            Situation::Fresh
        );
        assert_eq!(
            decide(&Facts {
                first_turn: true,
                new_to_the_work: true,
                ..base
            }),
            Situation::Handover,
            "a newcomer handed work starts from the handover"
        );
        assert_eq!(
            decide(&Facts {
                handed_a_note: true,
                ..base
            }),
            Situation::Handover
        );
        assert_eq!(
            decide(&Facts {
                recalled: true,
                handed_a_note: true,
                ..base
            }),
            Situation::Recalled
        );
    }

    #[test]
    fn each_role_and_brain_has_its_budget_and_the_dear_situations_more() {
        use Situation::{AfterGap, Continuing, Handover, Recalled};
        assert_eq!(budget(Role::Ceo, Brain::Opus, Continuing), 2_500);
        assert_eq!(budget(Role::Lead, Brain::Sonnet, Continuing), 4_000);
        assert_eq!(budget(Role::Developer, Brain::Haiku, Continuing), 3_000);
        assert_eq!(budget(Role::Reviewer, Brain::Sonnet, Continuing), 3_000);
        assert_eq!(budget(Role::Lead, Brain::Local, Continuing), 1_800);
        assert_eq!(budget(Role::Developer, Brain::Local, Continuing), 1_800);
        assert_eq!(budget(Role::Developer, Brain::Local, Handover), 3_000);
        assert_eq!(budget(Role::Lead, Brain::Sonnet, AfterGap), 5_200);
        assert_eq!(budget(Role::Ceo, Brain::Opus, Recalled), 3_700);
    }

    #[test]
    fn the_mac_has_its_own_budget_between_the_nas_models_and_claudes() {
        use Situation::{AfterGap, Continuing, Fresh, Handover, Recalled};
        // The Mac runs a large context, so the NAS's small one is not its limit; it is a model
        // in the house, so Claude's is not its to have either.
        for role in [
            Role::Ceo,
            Role::Lead,
            Role::Developer,
            Role::Reviewer,
            Role::Researcher,
        ] {
            for situation in [Continuing, Fresh, Handover, AfterGap, Recalled] {
                let nas = budget(role, Brain::Local, situation);
                let mac = budget(role, Brain::Mac, situation);
                let claude = budget(role, Brain::Sonnet, situation);
                assert!(
                    nas < mac && mac <= claude,
                    "{role:?} {situation:?}: the NAS's {nas}, the Mac's {mac}, Claude's {claude}"
                );
            }
        }
        assert_eq!(budget(Role::Lead, Brain::Mac, Continuing), 2_800);
        assert_eq!(budget(Role::Developer, Brain::Mac, Continuing), 2_800);
        assert_eq!(budget(Role::Reviewer, Brain::Mac, Fresh), 2_800);
        assert_eq!(
            budget(Role::Ceo, Brain::Mac, Continuing),
            2_500,
            "the CEO's briefings are held short on every brain"
        );
        assert_eq!(budget(Role::Developer, Brain::Mac, AfterGap), 4_000);
        // And the NAS's model keeps the budget it had.
        assert_eq!(budget(Role::Lead, Brain::Local, Continuing), LOCAL_BUDGET);
    }

    #[test]
    fn what_does_not_fit_is_named_never_cut_silently_and_the_core_is_never_cut() {
        let piece = |name: &str, chars: usize, weight| Piece::new(name, "x".repeat(chars), weight);
        // Everything fits: nothing is named.
        let all = compose(
            100,
            vec![
                piece("a", 40, Weight::Core),
                piece("b", 40, Weight::Optional),
            ],
        );
        assert_eq!(all.dropped, Vec::<String>::new());
        assert_eq!(all.omitted, "");
        assert_eq!(all.tokens, 20);
        // The optional go first, the biggest first; the recommended only after them all.
        let four = || {
            vec![
                piece("core", 200, Weight::Core),
                piece("rec", 200, Weight::Recommended),
                piece("small", 40, Weight::Optional),
                piece("big", 200, Weight::Optional),
            ]
        };
        let cut = compose(150, four());
        assert_eq!(cut.dropped, vec!["big".to_owned()]);
        assert_eq!(
            cut.briefed,
            vec!["core".to_owned(), "rec".to_owned(), "small".to_owned()]
        );
        assert!(cut.tokens <= 150, "{}", cut.tokens);
        let cut = compose(100, four());
        assert_eq!(
            cut.dropped,
            vec!["rec".to_owned(), "small".to_owned(), "big".to_owned()]
        );
        assert_eq!(cut.briefed, vec!["core".to_owned()]);
        assert!(cut.tokens <= 100, "{}", cut.tokens);
        assert!(!cut.over_budget);
        assert_eq!(
            cut.omitted,
            "Not included this turn, to keep to your briefing's budget: rec, small, big. Read \
             any of them with read_world (your notes with read_notebook).\n"
        );
        // The core alone is over: it all goes out, and that is said.
        let over = compose(
            10,
            vec![
                piece("core", 200, Weight::Core),
                piece("opt", 4, Weight::Optional),
            ],
        );
        assert!(over.over_budget);
        assert_eq!(over.briefed, vec!["core".to_owned()]);
        assert_eq!(over.dropped, vec!["opt".to_owned()]);
        assert!(over.text.len() == 200);
        // An empty piece is no piece.
        assert!(
            compose(10, vec![piece("none", 0, Weight::Core)])
                .briefed
                .is_empty()
        );
    }

    #[test]
    fn situations_keep_their_names() {
        for s in ALL {
            assert_eq!(Situation::from_name(s.name()), Some(s));
        }
        assert!(Situation::Handover.dear() && !Situation::Fresh.dear());
    }
}
