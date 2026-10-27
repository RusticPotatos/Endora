//! Goals' strategies and the leads' area reports (ADR 0120), as plain data and their rules.
//!
//! - A goal's **strategy** is the CEO's: its purpose ("the goal of the goal"), what success
//!   looks like, the measures the scorecard shows for it, its current bets (the active
//!   projects and why) and what was chosen not to do now. The owner edits it too.
//! - A lead's **area report** is its daily "state of my area": what moved, what is stuck,
//!   what it needs, and what it proposes. It is kept as a report, never as work on the board.

use super::work::{GoalId, MemberId, Millis, Refusal, RunId, TaskId};

/// The most characters one part of a strategy may run to.
pub const STRATEGY_PART_MOST: usize = 800;
/// The most characters one part of an area report may run to.
pub const REPORT_PART_MOST: usize = 600;
/// The title of a lead's daily report routine.
pub const REPORT_ROUTINE: &str = "State of my area";
/// The title of a lead's weekly patterns routine.
pub const PATTERNS_ROUTINE: &str = "Patterns";
/// The hour of the owner's day a lead's report is due.
pub const REPORT_HOUR: i64 = 18;

/// The measures HQ computes for a team, by name, as a strategy names them.
pub const MEASURES: [&str; 5] = [
    "unplanned outages over 10 min",
    "median alarm to closed",
    "repeat alarms within 24 h",
    "median branch submitted to push approved",
    "branches passing the tester first time",
];

/// A goal's strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    /// The goal it is for.
    pub goal: GoalId,
    /// Why the goal: the goal of the goal.
    pub purpose: String,
    /// What success looks like.
    pub success: String,
    /// The named numbers that say how it goes, one a line.
    pub measures: String,
    /// The current bets: the active projects, and why.
    pub bets: String,
    /// What was chosen not to do now.
    pub not_now: String,
    /// Who last wrote it.
    pub by: String,
    /// When.
    pub updated_at: Millis,
}

/// A change to a strategy: each part given replaces that part; `None` leaves it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StrategyEdit {
    /// The purpose.
    pub purpose: Option<String>,
    /// What success looks like.
    pub success: Option<String>,
    /// The measures.
    pub measures: Option<String>,
    /// The current bets.
    pub bets: Option<String>,
    /// What not now.
    pub not_now: Option<String>,
}

/// A part as kept: trimmed, control characters but line breaks left out, within its limit.
fn part(text: &str, most: usize) -> Result<String, Refusal> {
    let kept: String = text
        .trim()
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .collect();
    if kept.chars().count() > most {
        return Err(Refusal::TooLong(most));
    }
    Ok(kept)
}

impl Strategy {
    /// A goal's strategy before anything is written in it.
    #[must_use]
    pub const fn empty(goal: GoalId) -> Self {
        Self {
            goal,
            purpose: String::new(),
            success: String::new(),
            measures: String::new(),
            bets: String::new(),
            not_now: String::new(),
            by: String::new(),
            updated_at: 0,
        }
    }

    /// Whether nothing is written in it yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [
            &self.purpose,
            &self.success,
            &self.measures,
            &self.bets,
            &self.not_now,
        ]
        .iter()
        .all(|p| p.is_empty())
    }

    /// The strategy with `edit` applied, by `by` at `at`.
    ///
    /// # Errors
    /// [`Refusal::TooLong`] for a part past [`STRATEGY_PART_MOST`]; [`Refusal::NotAStrategy`]
    /// for an edit that changes nothing.
    pub fn edited(&self, edit: &StrategyEdit, by: &str, at: Millis) -> Result<Self, Refusal> {
        let given = [
            &edit.purpose,
            &edit.success,
            &edit.measures,
            &edit.bets,
            &edit.not_now,
        ];
        if given.iter().all(|p| p.is_none()) {
            return Err(Refusal::NotAStrategy);
        }
        let take = |new: &Option<String>, old: &str| match new {
            Some(text) => part(text, STRATEGY_PART_MOST),
            None => Ok(old.to_owned()),
        };
        Ok(Self {
            goal: self.goal,
            purpose: take(&edit.purpose, &self.purpose)?,
            success: take(&edit.success, &self.success)?,
            measures: take(&edit.measures, &self.measures)?,
            bets: take(&edit.bets, &self.bets)?,
            not_now: take(&edit.not_now, &self.not_now)?,
            by: by.to_owned(),
            updated_at: at,
        })
    }

    /// As a briefing says it, a part a line, the empty ones as "(not written)".
    #[must_use]
    pub fn said(&self) -> String {
        let line = |label: &str, text: &str| {
            let text = if text.is_empty() {
                "(not written)".to_owned()
            } else {
                text.replace('\n', "; ")
            };
            format!("  {label}: {text}\n")
        };
        [
            line("Purpose", &self.purpose),
            line("Success looks like", &self.success),
            line("Measures", &self.measures),
            line("Current bets", &self.bets),
            line("Not now", &self.not_now),
        ]
        .concat()
    }
}

/// A lead's report on its area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaReport {
    /// Its id.
    pub id: i64,
    /// The lead who wrote it.
    pub lead: MemberId,
    /// The turn it was written in.
    pub run: Option<RunId>,
    /// The report's task (a report routine's), if any.
    pub task: Option<TaskId>,
    /// What moved.
    pub moved: String,
    /// What is stuck.
    pub stuck: String,
    /// What the lead needs.
    pub need: String,
    /// What it proposes.
    pub proposals: String,
    /// When.
    pub at: Millis,
}

impl AreaReport {
    /// A report as a lead wrote it, each part within its limit.
    ///
    /// # Errors
    /// [`Refusal::TooLong`] for a part past [`REPORT_PART_MOST`]; [`Refusal::EmptyReport`]
    /// for one that says nothing.
    pub fn new(
        lead: MemberId,
        parts: [&str; 4],
        run: Option<RunId>,
        task: Option<TaskId>,
        at: Millis,
    ) -> Result<Self, Refusal> {
        let [moved, stuck, need, proposals] = parts;
        let report = Self {
            id: 0,
            lead,
            run,
            task,
            moved: part(moved, REPORT_PART_MOST)?,
            stuck: part(stuck, REPORT_PART_MOST)?,
            need: part(need, REPORT_PART_MOST)?,
            proposals: part(proposals, REPORT_PART_MOST)?,
            at,
        };
        if [
            &report.moved,
            &report.stuck,
            &report.need,
            &report.proposals,
        ]
        .iter()
        .all(|p| p.is_empty())
        {
            return Err(Refusal::EmptyReport);
        }
        Ok(report)
    }

    /// As the CEO's briefing says it: four short lines.
    #[must_use]
    pub fn said(&self) -> String {
        let line = |label: &str, text: &str| {
            let text = if text.is_empty() {
                "nothing".to_owned()
            } else {
                text.replace('\n', "; ")
            };
            format!("  {label}: {text}\n")
        };
        [
            line("Moved", &self.moved),
            line("Stuck", &self.stuck),
            line("Needs", &self.need),
            line("Proposes", &self.proposals),
        ]
        .concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strategy_is_edited_a_part_at_a_time_within_its_limits() {
        let empty = Strategy::empty(GoalId(3));
        assert!(empty.is_empty());
        assert_eq!(
            empty.edited(&StrategyEdit::default(), "Mara", 1),
            Err(Refusal::NotAStrategy)
        );
        let one = empty
            .edited(
                &StrategyEdit {
                    purpose: Some("  The house runs itself.\n".to_owned()),
                    ..StrategyEdit::default()
                },
                "Mara",
                5,
            )
            .unwrap();
        assert_eq!(
            (one.purpose.as_str(), one.by.as_str(), one.updated_at),
            ("The house runs itself.", "Mara", 5)
        );
        let two = one
            .edited(
                &StrategyEdit {
                    measures: Some(
                        "unplanned outages over 10 min\nmedian alarm to closed".to_owned(),
                    ),
                    ..StrategyEdit::default()
                },
                "owner",
                9,
            )
            .unwrap();
        assert_eq!(two.purpose, "The house runs itself.", "left as it was");
        assert!(
            two.said()
                .contains("  Measures: unplanned outages over 10 min; median alarm to closed\n")
        );
        assert!(two.said().contains("  Not now: (not written)\n"));
        assert_eq!(
            two.edited(
                &StrategyEdit {
                    bets: Some("x".repeat(STRATEGY_PART_MOST + 1)),
                    ..StrategyEdit::default()
                },
                "Mara",
                10
            ),
            Err(Refusal::TooLong(STRATEGY_PART_MOST))
        );
    }

    #[test]
    fn an_area_report_says_something_within_its_limits() {
        assert_eq!(
            AreaReport::new(MemberId(2), ["", " ", "", ""], None, None, 0),
            Err(Refusal::EmptyReport)
        );
        let report = AreaReport::new(
            MemberId(2),
            ["#667 closed", "", "a gluetun change", "pin gluetun"],
            None,
            None,
            0,
        )
        .unwrap();
        assert_eq!(
            report.said(),
            "  Moved: #667 closed\n  Stuck: nothing\n  Needs: a gluetun change\n  Proposes: pin gluetun\n"
        );
    }
}
