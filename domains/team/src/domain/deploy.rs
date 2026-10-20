//! The check after a deploy (ADR 0115, amended): once the owner says a pushed branch is
//! deployed, the member of the routine that takes the home lab's alarms takes one turn to read
//! the lab as it is now. That turn reads container health, logs and metrics and nothing else,
//! makes no work and ends in a verdict: healthy only on a reading of its own, cited; otherwise
//! a problem in its words, or inconclusive. These are the rules and the words; the engine
//! applies them.

use super::evidence::Criterion;

/// What a deploy check's title starts with; the branch's name follows.
pub const TITLE: &str = "Check the home lab after deploying ";

/// The node's tools a deploy check's turn may use: the lab's health, its logs and its metrics.
/// No act, no branch, no document, no file.
pub const LAB_READS: [&str; 3] = ["homelab_status", "container_logs", "nas_status"];

/// HQ's own tools it may use: to read its turn, take the task, say what it found and close it.
pub const HQ_TOOLS: [&str; 4] = ["my_turn", "claim", "post", "set_status"];

/// The setting that marks a task as a deploy check, by task: the branch's id is its value.
pub const MARK: &str = "deploy_check:";

/// The setting that marks a deploy check's verdict as given, by task, so it is given once.
pub const GIVEN: &str = "deploy_checked:";

/// The check's title for a branch.
#[must_use]
pub fn title(branch: &str) -> String {
    format!("{TITLE}{branch}")
}

/// What done means for the check: the lab as it is now, shown by a reading in the same turn.
#[must_use]
pub fn criterion() -> Criterion {
    Criterion::new(
        "A reading from this turn shows the containers the change touches running and healthy, \
         with no new errors in their logs since the deploy",
        true,
    )
}

/// What the check asks, with the change's own criteria quoted, so the member knows what the
/// branch was meant to do.
#[must_use]
pub fn description(branch: &str, task: i64, criteria: &[Criterion]) -> String {
    let mut said = format!(
        "The owner merged {branch} (task #{task}) and ran the playbook. Read the lab as it is \
         now with homelab_status, container_logs and nas_status, and say what you see. Close \
         this with set_status: done, citing a reading from this turn, if the lab is healthy; \
         blocked, saying the problem, if it is not or you cannot tell. You have this one turn."
    );
    if !criteria.is_empty() {
        said.push_str("\n\nWhat the change was meant to do:");
        for c in criteria {
            said.push_str(&format!("\n- {}", c.text));
        }
    }
    said
}

/// The owner's card once the branch is pushed.
#[must_use]
pub fn card(branch: &str) -> String {
    format!(
        "{branch} is on your fork. Once you have merged it and run the playbook, say Deployed: \
         the lab's routine member then checks the lab once, reading health, logs and metrics. \
         Won't deploy closes this."
    )
}

/// Why a deploy check's turn is refused a tool, as the model reads it.
#[must_use]
pub fn refused(tool: &str) -> String {
    format!(
        "{tool} is not for a post-deploy check: this turn reads the lab with homelab_status, \
         container_logs and nas_status, says what it saw with post, and closes the check with \
         set_status (done, citing a reading from this turn, or blocked, saying the problem)"
    )
}

/// What came of a deploy check's one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Closed done on a reading of the turn's own, cited: the receipts, as said.
    Healthy(String),
    /// Closed blocked: the problem, in the member's words.
    Problem(String),
    /// The turn ended without a verdict: why it ended.
    Inconclusive(String),
}

impl Verdict {
    /// The verdict in a line, for the change's thread and the owner's desk.
    #[must_use]
    pub fn said(&self, branch: &str) -> String {
        match self {
            Self::Healthy(evidence) => {
                format!("After the deploy: healthy. {branch} checked on the lab ({evidence}).")
            }
            Self::Problem(why) => format!("After the deploy: a problem with {branch}. {why}"),
            Self::Inconclusive(why) => format!(
                "After the deploy: inconclusive. The check of {branch} ended without a verdict \
                 ({why}); nothing shows the lab is as it should be."
            ),
        }
    }

    /// The desk's kind for it: a healthy deploy is for the digest; anything else interrupts.
    #[must_use]
    pub const fn desk_kind(&self) -> &'static str {
        match self {
            Self::Healthy(_) => "notice",
            Self::Problem(_) | Self::Inconclusive(_) => "deploy_check",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_reading_of_its_own_reads_healthy() {
        let healthy = Verdict::Healthy("r12 homelab_status".to_owned());
        assert!(healthy.said("hq/task-3-x").contains("healthy"));
        assert_eq!(healthy.desk_kind(), "notice");
        for not in [
            Verdict::Problem("deluge restarts".to_owned()),
            Verdict::Inconclusive("its turn ended".to_owned()),
        ] {
            assert!(!not.said("hq/task-3-x").contains("healthy"), "{not:?}");
            assert_eq!(not.desk_kind(), "deploy_check");
            assert!(super::super::desk::interrupts(not.desk_kind()));
        }
    }

    #[test]
    fn the_check_reads_and_never_acts() {
        assert!(criterion().live);
        for act in [
            "restart_container",
            "repo_checkout",
            "repo_submit",
            "ansible_nas_doc",
        ] {
            assert!(!LAB_READS.contains(&act), "{act}");
        }
        for makes_work in ["create_task", "hand_to", "ask_owner", "recheck", "review"] {
            assert!(!HQ_TOOLS.contains(&makes_work), "{makes_work}");
        }
    }
}
