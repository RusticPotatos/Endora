//! The chain of command (the owner's decision of 2026-10-17, ADR 0111). The owner is the
//! board; the CEO runs the team for them. A member's question goes up the chain, never to the
//! board's desk: the developer's, the researcher's and the reviewer's to the lead, the lead's to
//! the CEO. Whoever is asked answers in the task's thread, which wakes the one who asked. Only
//! the CEO puts anything to the board, and then as a decision with its recommendation. With no
//! CEO at work, the team works as it did before there was one.

use super::work::{Refusal, Role};

/// The start of a question asked up the chain, in the task's thread: `Question for Ada: …`.
pub const QUESTION_FOR: &str = "Question for ";

/// The most characters each part of a decision for the board may run to.
pub const DECISION_PART_MOST: usize = 400;

/// What only the board decides, as the CEO is told it every turn.
pub const BOARD_ITEMS: &str = "The owner is the board: put to them only what they alone \
    decide, each as a decision with your recommendation (ask_owner): money beyond the team's \
    pool; new goals, and what comes first; hiring or letting go a member; and anything only the \
    owner can know. Approvals for actions the owner set to ask reach their desk on their own. \
    Nothing operational goes to the board: decide it, or hand it to the lead with create_task.";

/// A question as it is posted in the task's thread, for the member it is asked of.
#[must_use]
pub fn question(to: &str, text: &str) -> String {
    format!("{QUESTION_FOR}{to}: {}", text.trim())
}

/// Who a member in `role` asks, up the chain: the lead for the developer, the researcher and
/// the reviewer (the CEO when there is no lead at work), and the CEO for the lead. `None` for
/// the CEO, whose questions go to the board, and for everyone while there is no CEO at work:
/// then the team works as it did before (to the reviewer first, then the owner's desk).
#[must_use]
pub const fn above(role: Role, a_lead_at_work: bool, a_ceo_at_work: bool) -> Option<Role> {
    if !a_ceo_at_work {
        return None;
    }
    match role {
        Role::Ceo => None,
        Role::Lead => Some(Role::Ceo),
        Role::Developer | Role::Researcher | Role::Reviewer if a_lead_at_work => Some(Role::Lead),
        Role::Developer | Role::Researcher | Role::Reviewer => Some(Role::Ceo),
    }
}

/// The questions in a thread (each post its author's name and its text, oldest first) asked of
/// `to` that `to` has not answered yet: who asked, and what. A question is answered by the
/// first post of `to`'s after it that is not itself a question up the chain or a draft for the
/// board, so a lead who asks the CEO in turn still owes the developer its answer.
#[must_use]
pub fn unanswered(thread: &[(&str, &str)], to: &str) -> Vec<(String, String)> {
    let asked = format!("{QUESTION_FOR}{to}: ");
    let mut open = Vec::new();
    for (i, (author, text)) in thread.iter().enumerate() {
        let Some(question) = text.strip_prefix(asked.as_str()) else {
            continue;
        };
        if *author == to {
            continue;
        }
        let answered = thread[i + 1..].iter().any(|(by, said)| {
            *by == to && !said.starts_with(QUESTION_FOR) && !said.starts_with(super::review::DRAFT)
        });
        if !answered {
            open.push(((*author).to_owned(), question.to_owned()));
        }
    }
    open
}

/// A decision the CEO puts to the board: what is to be decided, what it recommends, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// What is to be decided.
    pub decision: String,
    /// What the CEO recommends.
    pub recommendation: String,
    /// Why.
    pub why: String,
}

/// One line, as a part of a decision is kept.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

impl Decision {
    /// A decision from its three parts, each on one line.
    ///
    /// # Errors
    /// [`Refusal::NotADecision`] for a part left empty; [`Refusal::TooLong`] for one past
    /// [`DECISION_PART_MOST`] characters.
    pub fn new(decision: &str, recommendation: &str, why: &str) -> Result<Self, Refusal> {
        let parts = [decision, recommendation, why].map(one_line);
        if parts.iter().any(String::is_empty) {
            return Err(Refusal::NotADecision);
        }
        if parts.iter().any(|p| p.chars().count() > DECISION_PART_MOST) {
            return Err(Refusal::TooLong(DECISION_PART_MOST));
        }
        let [decision, recommendation, why] = parts;
        Ok(Self {
            decision,
            recommendation,
            why,
        })
    }

    /// As it is written in a thread and on the board's desk: three lines.
    #[must_use]
    pub fn text(&self) -> String {
        format!(
            "Decision: {}\nRecommendation: {}\nWhy: {}",
            self.decision, self.recommendation, self.why
        )
    }

    /// A decision read back from text that holds its three lines, wherever they are (a draft's
    /// opening words before the first are passed over), and `None` when one is missing.
    #[must_use]
    pub fn read(text: &str) -> Option<Self> {
        let mut parts: [Option<&str>; 3] = [None; 3];
        for line in text.lines() {
            let line = line.trim();
            let line = line
                .find("Decision: ")
                .filter(|_| parts[0].is_none())
                .map_or(line, |at| &line[at..]);
            for (i, label) in ["Decision: ", "Recommendation: ", "Why: "]
                .iter()
                .enumerate()
            {
                if let Some(said) = line.strip_prefix(label) {
                    parts[i] = parts[i].or(Some(said));
                }
            }
        }
        let [Some(decision), Some(recommendation), Some(why)] = parts else {
            return None;
        };
        Self::new(decision, recommendation, why).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_goes_up_the_chain_only_while_there_is_a_ceo() {
        for role in [Role::Developer, Role::Researcher, Role::Reviewer] {
            assert_eq!(above(role, true, true), Some(Role::Lead), "{role:?}");
            assert_eq!(
                above(role, false, true),
                Some(Role::Ceo),
                "{role:?} with no lead"
            );
            assert_eq!(above(role, true, false), None, "{role:?} with no CEO");
        }
        assert_eq!(above(Role::Lead, true, true), Some(Role::Ceo));
        assert_eq!(above(Role::Lead, true, false), None);
        assert_eq!(above(Role::Ceo, true, true), None, "the CEO asks the board");
    }

    #[test]
    fn a_question_waits_until_the_one_asked_says_something_that_is_not_a_question() {
        let thread = [
            ("Linus", "Restarted sonarr; it fails again."),
            ("Linus", "Question for Ada: may I raise its memory?"),
            ("Ada", "Question for Mara: may Linus raise sonarr's memory?"),
        ];
        assert_eq!(
            unanswered(&thread, "Ada"),
            [("Linus".to_owned(), "may I raise its memory?".to_owned())]
        );
        assert_eq!(
            unanswered(&thread, "Mara"),
            [(
                "Ada".to_owned(),
                "may Linus raise sonarr's memory?".to_owned()
            )]
        );
        let mut answered = thread.to_vec();
        answered.push(("Mara", "Draft for the owner: Decision: …"));
        assert_eq!(
            unanswered(&answered, "Mara").len(),
            1,
            "a draft is no answer"
        );
        answered.push(("Mara", "Yes, to 2 GB."));
        assert!(unanswered(&answered, "Mara").is_empty());
        assert_eq!(
            unanswered(&answered, "Ada").len(),
            1,
            "Ada still owes Linus"
        );
        answered.push(("Ada", "Yes: Mara agreed, 2 GB."));
        assert!(unanswered(&answered, "Ada").is_empty());
        assert!(unanswered(&answered, "Linus").is_empty());
        assert_eq!(
            question("Ada", "  may I? "),
            "Question for Ada: may I?",
            "as it is posted"
        );
    }

    #[test]
    fn a_decision_has_its_three_parts_on_one_line_each_and_reads_back() {
        let d = Decision::new(
            "Raise the team's pool to $90 a month?",
            "Yes, for\nOctober only.",
            "The lead ran out twice this week.",
        )
        .unwrap();
        assert_eq!(
            d.text(),
            "Decision: Raise the team's pool to $90 a month?\nRecommendation: Yes, for October \
             only.\nWhy: The lead ran out twice this week."
        );
        assert_eq!(Decision::read(&d.text()), Some(d.clone()));
        let drafted = format!(
            "Draft for the owner: {}\n\nWhat I actually did this turn: nothing.",
            d.text()
        );
        assert_eq!(Decision::read(&drafted), Some(d));
        assert_eq!(
            Decision::read("Decision: x\nWhy: y"),
            None,
            "no recommendation"
        );
        assert_eq!(Decision::new("x", " ", "y"), Err(Refusal::NotADecision));
        assert_eq!(
            Decision::new("x", "y", &"z".repeat(DECISION_PART_MOST + 1)),
            Err(Refusal::TooLong(DECISION_PART_MOST))
        );
        assert!(BOARD_ITEMS.contains("hiring") && BOARD_ITEMS.contains("pool"));
    }
}
