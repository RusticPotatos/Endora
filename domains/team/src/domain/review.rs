//! The reviewer: the one path work and questions take before they are done or reach the
//! owner's desk. Work moved to review, and a question about the home lab, wake the reviewer;
//! its verdict approves or sends back. A small model once put an invented container name and
//! an invented "ping test" on the owner's desk; these rules are why that is read by someone,
//! and flagged, first.

use super::work::{MemberId, Refusal};

/// The team's own tools, which look like Docker's names (`word_word`) but are not containers.
const TOOL_NAMES: &[&str] = &[
    "my_turn",
    "set_status",
    "create_task",
    "ask_owner",
    "check_back",
    "report_usage",
    "homelab_status",
    "container_logs",
    "restart_container",
    "homelab_map",
    "ansible_nas_doc",
    "review",
    "note_lesson",
    "note_house",
    "rewrite_house",
    "read_doc",
];

/// What the reviewer is told when work is put before it.
pub const REVIEW_WORK: &str = "You review it one rank up: re-check it yourself. Take your own \
    readings (homelab_status, container_logs, the queues) to confirm each piece of its evidence \
    is real and still true. For a branch of the wing's, your own reading is the branch itself: \
    check it out (repo_checkout, with its name, hq/task-N-<slug>, as the thread gives it), look \
    at what it changes in the shell (git diff against its base, git log, the files), and cite \
    that checkout's receipt with the check's. Pass only what holds (review, verdict approve), \
    citing your receipts per criterion: your pass closes it. Send it back with exactly what does not \
    hold (review, verdict send_back). Send back any problem that no tool shows causing harm.";

/// Whether work needs the gate (tests, lint and build on a runner) before a review can close
/// it: everything but a routine's task and work under a goal the home lab's routine serves,
/// which change no code.
#[must_use]
pub const fn needs_gate(a_routines_task: bool, the_home_labs: bool) -> bool {
    !(a_routines_task || the_home_labs)
}

/// Whether a reviewer may approve work: never its own, and work that needs the gate only once
/// the gate has passed.
///
/// # Errors
/// [`Refusal::OwnWork`] or [`Refusal::GateNotPassed`].
pub fn may_approve(
    needs_gate: bool,
    gate_passed: bool,
    worker: Option<MemberId>,
    reviewer: MemberId,
) -> Result<(), Refusal> {
    if worker == Some(reviewer) {
        return Err(Refusal::OwnWork);
    }
    if needs_gate && !gate_passed {
        return Err(Refusal::GateNotPassed);
    }
    Ok(())
}

/// The start of the post that holds a draft for the owner, in the task's thread.
pub const DRAFT: &str = "Draft for the owner: ";

/// Why work was sent back, when the reason is one HQ counts (ADR 0132). A send-back always says
/// what does not hold, in its notes; a reason names the kind of failure the measures count, so
/// that a fall in it shows a memory build working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendBackReason {
    /// It repeats what the team, the lab or the fork already has: the notes say where it is.
    ExistsAlready,
}

impl SendBackReason {
    /// Every reason a tester or a lead may give.
    pub const ALL: [Self; 1] = [Self::ExistsAlready];

    /// Stable lowercase name, as the tools take it and the feed keeps it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ExistsAlready => "exists_already",
        }
    }

    /// The reason with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }

    /// Every name, for a refusal to list: `exists_already`.
    #[must_use]
    pub fn names() -> String {
        Self::ALL
            .iter()
            .map(|r| r.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Whether a word could be a container's name: letters, digits, `_`, `.` and `-`, starting
/// with a letter or digit, at least one letter, at most 64 characters.
fn container_like(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && word.len() <= 64
        && word.chars().any(|c| c.is_ascii_alphabetic())
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Whether a bare word looks like a name Docker makes up: two runs of letters joined by one
/// underscore (`quirky_hopper`).
fn docker_like(word: &str) -> bool {
    let Some((a, b)) = word.split_once('_') else {
        return false;
    };
    !a.is_empty()
        && !b.is_empty()
        && a.chars().all(|c| c.is_ascii_alphabetic())
        && b.chars().all(|c| c.is_ascii_alphabetic())
}

/// The container-like names a text gives: each name in backticks that could be a container's,
/// and each bare word that looks like one Docker made up. The team's own tools are not
/// names. In order, each once.
#[must_use]
pub fn named(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut keep = |word: &str| {
        let word = word.trim_start_matches('/');
        if !TOOL_NAMES.iter().any(|t| t.eq_ignore_ascii_case(word))
            && !found.iter().any(|f| f.eq_ignore_ascii_case(word))
        {
            found.push(word.to_owned());
        }
    };
    for (i, quoted) in text.split('`').enumerate() {
        if i % 2 == 1 && container_like(quoted.trim_start_matches('/')) {
            keep(quoted);
        }
    }
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if docker_like(word) {
            keep(word);
        }
    }
    found
}

/// The home lab's containers a text names as whole words, case aside, in the lab's order.
fn lab_names_in(text: &str, lab: &[String]) -> Vec<String> {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
        .map(|w| {
            w.trim_matches(|c| c == '.' || c == '-')
                .to_ascii_lowercase()
        })
        .collect();
    lab.iter()
        .filter(|name| words.iter().any(|w| *w == name.to_ascii_lowercase()))
        .cloned()
        .collect()
}

/// The home lab's containers a task is about: those its title names, or, when its title names
/// none, those its description names. A title says what the work is; a description also names
/// what to leave alone. What one task is about, no second open task under its goal takes on
/// (see the engine's `create_task`).
#[must_use]
pub fn about(title: &str, description: &str, lab: &[String]) -> Vec<String> {
    let named = lab_names_in(title, lab);
    if !named.is_empty() {
        return named;
    }
    lab_names_in(description, lab)
}

/// The container-like names a text gives that the home lab does not have, in order.
#[must_use]
pub fn not_found(text: &str, lab: &[String]) -> Vec<String> {
    named(text)
        .into_iter()
        .filter(|n| !lab.iter().any(|l| l.eq_ignore_ascii_case(n)))
        .collect()
}

/// A question as it reaches the owner's desk: as it was written, with the names the home lab
/// does not have listed under it, never quietly taken out. With no snapshot of the home lab
/// (`None`) nothing can be judged, and the question is left as it is.
#[must_use]
pub fn checked(text: &str, lab: Option<&[String]>) -> String {
    let Some(lab) = lab else {
        return text.to_owned();
    };
    let missing = not_found(text, lab);
    if missing.is_empty() {
        return text.to_owned();
    }
    format!(
        "{text}\nNot found in Endora's home lab: {}",
        missing.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab() -> Vec<String> {
        ["sonarr", "radarr", "owntone-spotify", "quirky_hopper"]
            .map(str::to_owned)
            .to_vec()
    }

    #[test]
    fn container_like_names_are_the_backticked_ones_and_dockers_made_up_ones() {
        assert_eq!(
            named("Restart `Crappy_Cray`? Crappy_Cray failed a ping test; `sonarr` is fine."),
            ["Crappy_Cray", "sonarr"]
        );
        assert_eq!(
            named("I read `owntone-spotify` and `/radarr`."),
            ["owntone-spotify", "radarr"]
        );
        assert!(
            named("I used homelab_status and `container_logs`, then ask_owner.").is_empty(),
            "the team's tools are not containers"
        );
        assert!(
            named("Is `qwen3:8b` right for `docker compose up`? Port `8080`.").is_empty(),
            "a model, a command and a number are not containers"
        );
        assert!(named("sonarr_remove_and_search is not Docker's shape").is_empty());
    }

    #[test]
    fn a_task_is_about_the_containers_its_title_names_or_else_its_description() {
        let lab: Vec<String> = ["deluge", "nzbget", "prowlarr", "owntone", "owntone-spotify"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            about(
                "Diagnose and fix exited Deluge, nzbget, prowlarr",
                "Leave owntone and owntone-spotify alone.",
                &lab
            ),
            ["deluge", "nzbget", "prowlarr"],
            "the title says what it is about; the description may name what to leave"
        );
        assert_eq!(
            about("Check the stopped ones", "Look at owntone-spotify.", &lab),
            ["owntone-spotify"],
            "a whole name, never a part of a longer one"
        );
        assert!(about("Check the home lab", "Look at it all.", &lab).is_empty());
        assert!(about("Fix deluge", "", &[]).is_empty(), "no lab, no names");
    }

    #[test]
    fn a_name_the_lab_does_not_have_is_listed_under_the_question_never_removed() {
        let said = "Crappy_Cray failed a ping test. Restart `sonarr` and `Crappy_Cray`?";
        assert_eq!(
            checked(said, Some(&lab())),
            format!("{said}\nNot found in Endora's home lab: Crappy_Cray")
        );
        assert_eq!(
            checked("Restart `SONARR`?", Some(&lab())),
            "Restart `SONARR`?"
        );
        assert_eq!(
            checked(said, None),
            said,
            "with no snapshot nothing is judged"
        );
    }

    /// Live (task 670): the tester was never told how to read a branch, and could not, so no
    /// pass of hers was ever recorded. Her reading of a wing branch is checking it out.
    #[test]
    fn the_reviewer_is_told_to_read_a_branch_by_checking_it_out_and_to_cite_that_receipt() {
        assert!(
            REVIEW_WORK.contains("repo_checkout") && REVIEW_WORK.contains("hq/task-N-<slug>"),
            "{REVIEW_WORK}"
        );
        assert!(
            REVIEW_WORK.contains("cite that checkout's receipt with the check's"),
            "{REVIEW_WORK}"
        );
        // What the home lab's reviews already said stays.
        assert!(
            REVIEW_WORK.starts_with(
                "You review it one rank up: re-check it yourself. Take your own readings \
                 (homelab_status, container_logs, the queues)"
            ) && REVIEW_WORK.ends_with("Send back any problem that no tool shows causing harm."),
            "{REVIEW_WORK}"
        );
        // She does not submit what she reads.
        assert!(!REVIEW_WORK.contains("repo_submit"));
    }

    #[test]
    fn a_review_closes_ungated_work_but_never_its_own_and_gated_work_waits_for_the_gate() {
        assert!(!needs_gate(true, false), "a routine's task");
        assert!(!needs_gate(false, true), "the home lab's work");
        assert!(needs_gate(false, false));
        let (me, other) = (MemberId(1), MemberId(2));
        assert_eq!(may_approve(false, false, Some(other), me), Ok(()));
        assert_eq!(
            may_approve(false, true, Some(me), me),
            Err(Refusal::OwnWork)
        );
        assert_eq!(
            may_approve(true, false, Some(other), me),
            Err(Refusal::GateNotPassed)
        );
        assert_eq!(may_approve(true, true, Some(other), me), Ok(()));
    }

    #[test]
    fn a_send_back_names_its_reason_from_a_short_set_the_measures_count() {
        assert_eq!(SendBackReason::ExistsAlready.name(), "exists_already");
        for reason in SendBackReason::ALL {
            assert_eq!(SendBackReason::from_name(reason.name()), Some(reason));
        }
        assert_eq!(SendBackReason::from_name("boring"), None);
        assert_eq!(SendBackReason::from_name(""), None);
        assert_eq!(SendBackReason::names(), "exists_already");
    }
}
