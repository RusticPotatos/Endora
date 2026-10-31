//! Which of the lab's tools a turn is refused, and HQ's own four (the Holodeck plan's phase 4;
//! the baseline experiment of 28-29 Oct). The check after a deploy that these began with
//! retired with the loops (ADR 0142): the lab's recurring health check is the check.

/// The node's tools a turn is refused when its task leaves the lab as it is: the restart.
pub const LAB_ACTS: [&str; 1] = ["restart_container"];

/// Whether a task's words say the lab is to be left as it is ("read only", "change nothing",
/// "nothing is changed on the lab", "don't change anything"). Two such tasks restarted
/// containers in the baseline (28-29 Oct): their turns are refused [`LAB_ACTS`].
#[must_use]
pub fn leaves_the_lab_alone(words: &str) -> bool {
    let words = words
        .to_lowercase()
        .replace(['\u{2019}', '-'], " ")
        .replace('\'', " ");
    [
        "read only",
        "change nothing",
        "nothing is changed",
        "nothing changed",
        "don t change anything",
        "do not change anything",
        "without changing anything",
    ]
    .iter()
    .any(|said| words.contains(said))
}

/// HQ's own tools it may use: to read its turn, take the task, say what it found and close it.
pub const HQ_TOOLS: [&str; 4] = ["my_turn", "claim", "post", "set_status"];

#[cfg(test)]
mod tests {
    use super::leaves_the_lab_alone;

    #[test]
    fn a_task_that_leaves_the_lab_alone_is_known_by_its_words() {
        for said in [
            "Read only: no restarts, no settings changes, no writes.",
            "read-only on the lab",
            "Nothing is changed on the lab",
            "change nothing on the running lab",
            "Investigate with the lab's logs, propose the fix; don\u{2019}t change anything.",
        ] {
            assert!(leaves_the_lab_alone(said), "{said}");
        }
        for said in [
            "Restart sonarr if it is unhealthy",
            "Fix the lint",
            "only read the docs",
        ] {
            assert!(!leaves_the_lab_alone(said), "{said}");
        }
    }
}
