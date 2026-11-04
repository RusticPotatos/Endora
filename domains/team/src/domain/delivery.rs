//! How accepted work reaches the owner's fork (ADR 0115, amended 2026-10-31). The owner decided
//! on 31 Oct that merging accepted work needs no review of theirs: an accepted branch is listed,
//! not carded, and the node pushes it once the code checks below pass. The rules here are pure:
//! when a branch may be pushed, which statuses it goes through, and what the owner reads when a
//! scan holds it or a change needs them. The scan itself runs on the node, which holds the
//! branch's whole diff and the owner's own values.

/// The setting that puts the "Ready to merge" and "Push" cards back on the desk: `on` for an
/// owner who wants to review again. Off (absent) by default.
pub const CARDS: &str = "delivery_cards";
/// The setting that has HQ merge a pushed branch into the fork's develop: `on`, or off (absent)
/// by default. Off while the baseline experiment's rounds run (develop is frozen).
pub const MERGE_DEVELOP: &str = "delivery_merge_develop";
/// The setting, by branch id, that keeps when a branch was accepted and why it is where it is.
pub const KEPT: &str = "delivery:";

/// Accepted and passed: waiting for the node to scan and push it.
pub const DUE: &str = "due";
/// The owner read what the scan found and said to push it anyway: pushed without the scan.
pub const DUE_UNSCANNED: &str = "due_unscanned";
/// Accepted, but not pushed by rule (no deploy key, or a check not passed): listed, with why.
pub const ACCEPTED: &str = "accepted";
/// The privacy scan found something: nothing was pushed.
pub const HELD: &str = "held";
/// Pushed to the fork, waiting to merge.
pub const ON_FORK: &str = "on_fork";
/// The push failed: listed with what git said.
pub const PUSH_FAILED: &str = "push_failed";
/// Merged into the fork's develop: off the list.
pub const MERGED: &str = "merged";
/// The merge failed (a conflict, or develop moved): listed, for the owner's own hand.
pub const MERGE_FAILED: &str = "merge_failed";

/// A submission whose check passed, kept for the owner when its item ended blocked before any
/// pass (D22 of round 3): not accepted and never pushed by HQ, but listed, its bundle a download
/// away, rather than left only in the node's mirror.
pub const KEPT_BLOCKED: &str = "kept_blocked";

/// The statuses Manage lists as "Accepted, waiting to merge".
pub const WAITING: [&str; 8] = [
    DUE,
    DUE_UNSCANNED,
    ACCEPTED,
    HELD,
    ON_FORK,
    PUSH_FAILED,
    MERGE_FAILED,
    KEPT_BLOCKED,
];

/// Whether a branch in this status is listed as waiting to merge.
#[must_use]
pub fn waiting(status: &str) -> bool {
    WAITING.contains(&status)
}

/// A status in a few words, as the list shows it.
#[must_use]
pub fn said(status: &str) -> &'static str {
    match status {
        DUE => "pushing after its privacy scan",
        DUE_UNSCANNED => "pushing, as you said",
        ACCEPTED => "not pushed",
        HELD => "held by the privacy scan",
        ON_FORK => "on the fork",
        PUSH_FAILED => "push failed",
        MERGE_FAILED => "merge failed",
        MERGED => "merged into develop",
        KEPT_BLOCKED => "kept: its item ended blocked, not reviewed or pushed",
        _ => "",
    }
}

/// Why an accepted branch of a repository outside the lab is not pushed (ADR 0115, amended
/// 2026-11-01): it is kept on the node for judging, and never pushed anywhere.
pub const OUTSIDE_KEPT: &str =
    "a branch of a repository outside the lab is kept on the node for judging: it is never pushed";

/// What a branch's checks came to, as HQ reads them when its work is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passed<'a> {
    /// Whether the node holds a deploy key to push with.
    pub can_push: bool,
    /// The newest check of the branch: passed, failed, or none finished.
    pub check: Option<bool>,
    /// Files the branch touches that nobody touches (HQ's mechanical checks).
    pub forbidden: &'a [String],
    /// Whether a criterion asks for a CI run that no workflow can give (HQ's mechanical checks).
    pub no_ci_run: bool,
}

/// Whether an accepted branch may be pushed without a card: its review passed (it is accepted),
/// its check passed, and HQ's mechanical checks found no forbidden path and no CI run that
/// cannot be had. Each refusal says why, for the list. The privacy scan is the node's, after.
///
/// # Errors
/// Why it is not pushed.
pub fn may_push(passed: &Passed<'_>) -> Result<(), String> {
    if !passed.forbidden.is_empty() {
        return Err(format!(
            "it touches {}, which nobody touches",
            passed.forbidden.join(", ")
        ));
    }
    if passed.no_ci_run {
        return Err("a criterion asks for a CI run no workflow gives".to_owned());
    }
    match passed.check {
        Some(true) => {}
        Some(false) => return Err("its check failed".to_owned()),
        None => return Err("no check of it passed".to_owned()),
    }
    if !passed.can_push {
        return Err(
            "pushing is not set up on the node (ENDORA_GIT_PUSH_KEY_FILE names no deploy key): \
             its bundle can be downloaded"
                .to_owned(),
        );
    }
    Ok(())
}

/// One thing a scan found, and where: never the value itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// What kind of thing: "a real network address", "one of your own values", or the
    /// variable a change needs.
    pub what: String,
    /// Where: a file and line, or a commit's message.
    pub at: String,
}

/// The most findings a card names; the rest are counted.
pub const NAMED_MOST: usize = 6;

/// Findings in a line: "what (where), what (where) and N more".
#[must_use]
pub fn listed(found: &[Found]) -> String {
    let cut = |s: &str, most: usize| -> String { s.chars().take(most).collect() };
    let mut said = found
        .iter()
        .take(NAMED_MOST)
        .map(|f| format!("{} ({})", cut(&f.what, 80), cut(&f.at, 160)))
        .collect::<Vec<_>>()
        .join(", ");
    if found.len() > NAMED_MOST {
        said.push_str(&format!(" and {} more", found.len() - NAMED_MOST));
    }
    said
}

/// The card when the privacy scan holds a branch: what and where, never the value.
#[must_use]
pub fn held_said(branch: &str, found: &[Found]) -> String {
    format!(
        "Not pushed: the privacy scan of {branch} found {}. Nothing left the house. Send it \
         back to have the team take it out, or push it anyway if it is not yours.",
        listed(found)
    )
}

/// The card when an accepted change needs something of the owner.
#[must_use]
pub fn needs_said(branch: &str, needs: &[Found]) -> String {
    format!(
        "{branch} needs something of you before it is deployed: {}. Set it where your own \
         values live (the vault), then mark this seen.",
        listed(needs)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passed() -> Passed<'static> {
        Passed {
            can_push: true,
            check: Some(true),
            forbidden: &[],
            no_ci_run: false,
        }
    }

    #[test]
    fn a_branch_that_passed_everything_may_be_pushed() {
        assert_eq!(may_push(&passed()), Ok(()));
    }

    #[test]
    fn a_failed_or_missing_check_a_forbidden_path_or_no_ci_run_holds_it() {
        let forbidden = ["group_vars/all.yml".to_owned()];
        for (case, why) in [
            (
                Passed {
                    check: Some(false),
                    ..passed()
                },
                "check failed",
            ),
            (
                Passed {
                    check: None,
                    ..passed()
                },
                "no check",
            ),
            (
                Passed {
                    forbidden: &forbidden,
                    ..passed()
                },
                "group_vars/all.yml",
            ),
            (
                Passed {
                    no_ci_run: true,
                    ..passed()
                },
                "CI run",
            ),
            (
                Passed {
                    can_push: false,
                    ..passed()
                },
                "not set up",
            ),
        ] {
            let said = may_push(&case).unwrap_err();
            assert!(said.contains(why), "{said}");
        }
    }

    #[test]
    fn the_list_holds_what_waits_and_not_what_merged_or_the_old_cards_flow() {
        for s in WAITING {
            assert!(waiting(s), "{s}");
            assert!(!said(s).is_empty(), "{s}");
        }
        for s in [MERGED, "offered", "approved", "pushed", "submitted"] {
            assert!(!waiting(s), "{s}");
        }
    }

    #[test]
    fn a_held_card_says_what_and_where_and_counts_past_six() {
        let found: Vec<Found> = (1..=8)
            .map(|n| Found {
                what: "a real mailbox".to_owned(),
                at: format!("docs/a.md line {n}"),
            })
            .collect();
        let said = held_said("hq/task-1-x", &found);
        assert!(said.starts_with("Not pushed: the privacy scan of hq/task-1-x found a real mailbox (docs/a.md line 1)"), "{said}");
        assert!(said.contains("and 2 more"), "{said}");
        assert!(!said.contains("line 7"), "{said}");
    }
}
