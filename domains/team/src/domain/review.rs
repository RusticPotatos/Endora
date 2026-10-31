//! The reviewer: the one path work and questions take before they are done or reach the
//! owner's desk. Work moved to review, and a question about the home lab, wake the reviewer;
//! its verdict approves or sends back. A small model once put an invented container name and
//! an invented "ping test" on the owner's desk; these rules are why that is read by someone,
//! and flagged, first.

use super::work::MemberId;

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
    "image_tags",
    "review",
    "note_lesson",
    "note_house",
    "rewrite_house",
    "read_doc",
];

/// The highest rank any review is given at (ADR 0142, amended 2026-10-31, D7): Haiku's.
/// Sonnet's and Opus's reviews were most of round 2's spend; harder work reaches stronger models
/// by its climb, not by review.
pub const REVIEWS_CAPPED_AT: u8 = super::Brain::Haiku.rank();

/// Who reviews the work `worker` did at `rank`, among the loops' `members` (each with its
/// rank; 1 is the most able), never the worker itself (ADR 0142, amended 2026-10-31, D7).
///
/// Work done below Haiku's rank is reviewed one rank up but never above Haiku's: the nearest
/// member above it at Haiku's rank or below, else the nearest below it, else the next member up
/// the ranks. Work done at Haiku's rank or above is reviewed by another member at Haiku's rank,
/// else the nearest member below Haiku's rank, else, as the last resort, the nearest above it.
/// Ties go to the member hired first.
#[must_use]
pub fn reviewer_among(worker: MemberId, rank: u8, members: &[(MemberId, u8)]) -> Option<MemberId> {
    let others: Vec<(MemberId, u8)> = members
        .iter()
        .copied()
        .filter(|(id, _)| *id != worker)
        .collect();
    let nearest_above = |of: u8, highest: u8| {
        others
            .iter()
            .filter(|(_, r)| *r < of && *r >= highest)
            .max_by_key(|(id, r)| (*r, std::cmp::Reverse(id.0)))
            .map(|(id, _)| *id)
    };
    let nearest_below = |of: u8| {
        others
            .iter()
            .filter(|(_, r)| *r > of)
            .min_by_key(|(id, r)| (*r, id.0))
            .map(|(id, _)| *id)
    };
    if rank > REVIEWS_CAPPED_AT {
        return nearest_above(rank, REVIEWS_CAPPED_AT)
            .or_else(|| nearest_below(rank))
            .or_else(|| nearest_above(rank, 0));
    }
    let at_the_cap = others
        .iter()
        .filter(|(_, r)| *r == REVIEWS_CAPPED_AT)
        .min_by_key(|(id, _)| id.0)
        .map(|(id, _)| *id);
    at_the_cap
        .or_else(|| nearest_below(REVIEWS_CAPPED_AT))
        .or_else(|| nearest_above(REVIEWS_CAPPED_AT, 0))
}

/// What the reviewer is told when work is put before it.
pub const REVIEW_WORK: &str = "You review another member's work: re-check it yourself. Take your own \
    readings (homelab_status, container_logs, the queues) to confirm each piece of its evidence \
    is real and still true. For a branch of the wing's, your own reading is the branch itself: \
    check it out (repo_checkout, with its name, hq/task-N-<slug>, as the thread gives it), look \
    at what it changes in the shell (git diff against its base, git log, the files), and cite \
    that checkout's receipt with the check's. Pass only what holds (review, verdict approve), \
    citing your receipts per criterion: your pass closes it. Send it back with exactly what does not \
    hold (review, verdict send_back). Send back any problem that no tool shows causing harm.";

/// The most of a branch's diff a reviewer's briefing carries, in characters (ADR 0142, amended
/// 2026-10-30): a little on the house's models, which pay for every line on every call, more on
/// Claude; the whole diff is a `read_document` away, and the branch a `repo_checkout`.
#[must_use]
pub const fn diff_chars_for(brain: super::Brain) -> usize {
    if brain.cloud() { 12_000 } else { 2_400 }
}

/// The most of a branch's changed files a reviewer's briefing names.
pub const FILES_MOST: usize = 20;

/// A diff within `most` characters, cut at a line's end, and whether it was cut.
#[must_use]
pub fn diff_within(diff: &str, most: usize) -> (String, bool) {
    if diff.chars().count() <= most {
        return (diff.to_owned(), false);
    }
    let mut kept = String::new();
    for line in diff.split_inclusive('\n') {
        if kept.chars().count() + line.chars().count() > most {
            break;
        }
        kept.push_str(line);
    }
    (kept, true)
}

/// Whether work needs the gate (tests, lint and build on a runner) before a review can close
/// it: everything but a routine's task and work under a goal the home lab's routine serves,
/// which change no code.
#[must_use]
pub const fn needs_gate(a_routines_task: bool, the_home_labs: bool) -> bool {
    !(a_routines_task || the_home_labs)
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

    /// Mara (Opus), Kit (Sonnet), Ada (Haiku), Rae (DeepSeek), Mo (the Mac), Bo (the house's).
    const MARA: MemberId = MemberId(1);
    const KIT: MemberId = MemberId(2);
    const ADA: MemberId = MemberId(3);
    const RAE: MemberId = MemberId(4);
    const MO: MemberId = MemberId(5);
    const BO: MemberId = MemberId(6);

    fn ranked(members: &[MemberId]) -> Vec<(MemberId, u8)> {
        let rank = |id: MemberId| match id.0 {
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 5,
            _ => 6,
        };
        members.iter().map(|m| (*m, rank(*m))).collect()
    }

    /// Cy, Dee and Zed, on Haiku too, hired after Ada and in that order.
    const CY: MemberId = MemberId(7);
    const DEE: MemberId = MemberId(8);
    const ZED: MemberId = MemberId(9);

    #[test]
    fn work_at_haiku_s_rank_or_above_is_reviewed_by_another_member_at_haiku_s_rank() {
        let mut team = ranked(&[MARA, KIT, ADA, RAE, MO, BO]);
        team.extend([(CY, 3), (DEE, 3), (ZED, 3)]);
        // Sonnet's and Opus's work: the Haiku member hired first.
        assert_eq!(reviewer_among(KIT, 2, &team), Some(ADA));
        assert_eq!(reviewer_among(MARA, 1, &team), Some(ADA));
        // Haiku's work: another member at Haiku's rank, never itself, the one hired first.
        assert_eq!(reviewer_among(ADA, 3, &team), Some(CY));
        assert_eq!(reviewer_among(CY, 3, &team), Some(ADA));
        assert_eq!(reviewer_among(ZED, 3, &team), Some(ADA));
    }

    #[test]
    fn with_nobody_else_at_haiku_s_rank_the_nearest_below_it_reviews_else_the_nearest_above() {
        // Ada the only Haiku member: her work goes to the nearest below, Rae, not to Kit.
        let all = ranked(&[MARA, KIT, ADA, RAE, MO, BO]);
        assert_eq!(reviewer_among(ADA, 3, &all), Some(RAE));
        let no_rae = ranked(&[MARA, KIT, ADA, MO, BO]);
        assert_eq!(reviewer_among(ADA, 3, &no_rae), Some(MO));
        // No Haiku member: Sonnet's and Opus's work, the nearest below Haiku's rank.
        let no_haiku = ranked(&[MARA, KIT, RAE, MO, BO]);
        assert_eq!(reviewer_among(KIT, 2, &no_haiku), Some(RAE));
        assert_eq!(reviewer_among(MARA, 1, &no_haiku), Some(RAE));
        // Nobody else at Haiku's rank or below: the nearest above, as the last resort.
        let ada_alone = ranked(&[MARA, KIT, ADA]);
        assert_eq!(reviewer_among(ADA, 3, &ada_alone), Some(KIT));
        let top_two = ranked(&[MARA, KIT]);
        assert_eq!(reviewer_among(KIT, 2, &top_two), Some(MARA));
        assert_eq!(reviewer_among(MARA, 1, &top_two), Some(KIT));
        assert_eq!(reviewer_among(MARA, 1, &ranked(&[MARA])), None);
    }

    #[test]
    fn work_below_haiku_s_rank_is_reviewed_one_rank_up_when_that_is_within_the_cap() {
        let all = ranked(&[MARA, KIT, ADA, RAE, MO, BO]);
        assert_eq!(reviewer_among(BO, 6, &all), Some(MO));
        assert_eq!(reviewer_among(MO, 5, &all), Some(RAE));
        assert_eq!(reviewer_among(RAE, 4, &all), Some(ADA));
    }

    #[test]
    fn work_below_haiku_s_rank_is_never_reviewed_above_it() {
        // No Haiku on the team: Rae's work goes to the Mac's member below her, not to Kit.
        let no_haiku = ranked(&[MARA, KIT, RAE, MO, BO]);
        assert_eq!(reviewer_among(RAE, 4, &no_haiku), Some(MO));
        // Nobody between Bo and Haiku's rank but the Mac's member: still the Mac's member.
        assert_eq!(reviewer_among(BO, 6, &no_haiku), Some(MO));
        // Nobody above the Mac's member within the cap: Bo, below, rather than Kit.
        let mac_and_bo = ranked(&[MARA, KIT, MO, BO]);
        assert_eq!(reviewer_among(MO, 5, &mac_and_bo), Some(BO));
    }

    #[test]
    fn a_member_never_reviews_its_own_work_and_the_next_one_up_takes_it() {
        // The only member at Haiku's rank or below is the worker: the next member up the ranks.
        let rae_alone = ranked(&[MARA, KIT, RAE]);
        assert_eq!(reviewer_among(RAE, 4, &rae_alone), Some(KIT));
        // Alone on the team, nobody reviews.
        assert_eq!(reviewer_among(BO, 6, &ranked(&[BO])), None);
        // A second member at the worker's own rank does not count as one rank up.
        let two_bos = [(BO, 6), (MemberId(7), 6), (MARA, 1)];
        assert_eq!(reviewer_among(BO, 6, &two_bos), Some(MARA));
    }

    #[test]
    fn reviews_are_capped_at_haiku_s_rank() {
        assert_eq!(REVIEWS_CAPPED_AT, 3);
    }

    #[test]
    fn a_diff_is_cut_at_a_line_s_end_within_its_size() {
        let diff = "+one\n+two\n+three\n";
        assert_eq!(diff_within(diff, 100), (diff.to_owned(), false));
        assert_eq!(diff_within(diff, 10), ("+one\n+two\n".to_owned(), true));
        assert_eq!(diff_within(diff, 3), (String::new(), true));
    }

    #[test]
    fn the_house_s_models_read_less_of_a_diff_than_claude() {
        use super::super::Brain;
        assert!(diff_chars_for(Brain::Local) < diff_chars_for(Brain::Sonnet));
        assert_eq!(diff_chars_for(Brain::Local), diff_chars_for(Brain::Mac));
    }

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
                "You review another member's work: re-check it yourself. Take your own readings \
                 (homelab_status, container_logs, the queues)"
            ) && REVIEW_WORK.ends_with("Send back any problem that no tool shows causing harm."),
            "{REVIEW_WORK}"
        );
        // She does not submit what she reads.
        assert!(!REVIEW_WORK.contains("repo_submit"));
    }

    #[test]
    fn routine_and_home_lab_work_needs_no_gate() {
        assert!(!needs_gate(true, false), "a routine's task");
        assert!(!needs_gate(false, true), "the home lab's work");
        assert!(needs_gate(false, false));
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
