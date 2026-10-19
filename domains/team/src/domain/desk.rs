//! What waits on the owner (ADR 0132): which kinds of desk item interrupt them, and which are
//! left for the "while you were away" digest.

/// The kinds the digest holds: stuck work, a guard's finding and a notice. Each is something the
/// owner may want to look at, and none needs a decision before the team can go on.
pub const DIGEST: [&str; 3] = ["stuck", "guard", "notice"];

/// Whether an item of this kind interrupts the owner: it needs their decision, approval, answer
/// or money. Every kind but the digest's does, so a kind added later interrupts until it is
/// said otherwise, and a decision is never quietly folded into a digest.
#[must_use]
pub fn interrupts(kind: &str) -> bool {
    !DIGEST.contains(&kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind the owner's desk has held, with whether it interrupts. A new kind is added here
    /// when it is added to the desk.
    const KINDS: [(&str, bool); 18] = [
        ("goal", true),
        ("decision", true),
        ("approval", true),
        ("question", true),
        ("push", true),
        ("ready_to_merge", true),
        ("ready_to_read", true),
        ("rounds", true),
        ("hire", true),
        ("furlough", true),
        ("recall", true),
        ("canon", true),
        ("project_cap", true),
        ("project_budget", true),
        ("out_for_day", true),
        ("stuck", false),
        ("guard", false),
        ("notice", false),
    ];

    #[test]
    fn decisions_approvals_questions_pushes_and_money_interrupt_and_the_digest_does_not() {
        for (kind, interrupting) in KINDS {
            assert_eq!(interrupts(kind), interrupting, "{kind}");
        }
    }

    #[test]
    fn the_digest_is_exactly_the_kinds_that_do_not_interrupt() {
        let quiet: Vec<&str> = KINDS
            .iter()
            .filter(|(_, interrupting)| !interrupting)
            .map(|(kind, _)| *kind)
            .collect();
        assert_eq!(quiet, DIGEST);
    }

    #[test]
    fn a_kind_nobody_has_classified_interrupts() {
        assert!(interrupts("a_kind_from_the_future"));
    }
}
