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
    "run_gate",
    "request_action",
    "report_usage",
    "homelab_status",
    "container_logs",
    "restart_container",
    "review",
    "note_lesson",
];

/// What the reviewer is told when work is put before it.
pub const REVIEW_WORK: &str = "Check every claim in the thread against your tools \
    (homelab_status, container_logs, the queues). Approve only what holds (review, verdict \
    approve); send it back with exactly what does not hold (review, verdict send_back). Send \
    back any problem that no tool shows causing harm.";

/// What the reviewer is told when a draft for the owner is put before it.
pub const REVIEW_DRAFT: &str = "Check every claim in the draft against your tools \
    (homelab_status, container_logs, the queues). If everything holds, call ask_owner with the \
    draft, corrected if needed. If not, send it back to its owner with exactly what does not \
    hold (review, verdict send_back). Send back any problem that no tool shows causing harm. \
    Send back, never forward, a draft that asks the owner for something the team's own tools \
    could find (what is stuck, why, what a log says): the team finds it, the owner decides.";

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

/// Whether a text speaks of containers: it gives a container-like name, or names one the
/// home lab has as a whole word.
#[must_use]
pub fn names_containers(text: &str, lab: &[String]) -> bool {
    if !named(text).is_empty() {
        return true;
    }
    let words: Vec<String> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
        .map(|w| {
            w.trim_matches(|c| c == '.' || c == '-')
                .to_ascii_lowercase()
        })
        .collect();
    lab.iter()
        .any(|name| words.iter().any(|w| *w == name.to_ascii_lowercase()))
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

/// Whether a member's question goes to the reviewer before the owner's desk: when there is a
/// reviewer, the one asking is not it, and the task is the home lab's (under a goal whose
/// routine takes the home lab's alarms) or the question speaks of containers.
#[must_use]
pub const fn goes_to_review(
    asked_by_the_reviewer: bool,
    there_is_a_reviewer: bool,
    the_home_labs: bool,
    speaks_of_containers: bool,
) -> bool {
    !asked_by_the_reviewer && there_is_a_reviewer && (the_home_labs || speaks_of_containers)
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
    fn a_question_speaks_of_containers_when_it_names_one_the_lab_has_or_looks_like_one() {
        assert!(names_containers("sonarr keeps restarting.", &lab()));
        assert!(names_containers("Is owntone-spotify needed?", &lab()));
        assert!(names_containers("What about `jellyfin`?", &[]));
        assert!(!names_containers("Steel or titanium?", &lab()));
        assert!(!names_containers(
            "Shall I re-sonarr it?",
            &["sonar".to_owned()]
        ));
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

    #[test]
    fn a_draft_goes_to_review_unless_the_reviewer_asks_or_there_is_none() {
        assert!(goes_to_review(false, true, true, false));
        assert!(goes_to_review(false, true, false, true));
        assert!(!goes_to_review(false, true, false, false));
        assert!(
            !goes_to_review(true, true, true, true),
            "the reviewer's own"
        );
        assert!(!goes_to_review(false, false, true, true), "no reviewer");
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
}
