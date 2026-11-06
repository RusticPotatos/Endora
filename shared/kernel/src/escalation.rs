//! When to ask the owner (climb 2, S3): one statement in one place, rendered into HQ's
//! `ask_owner` tool and the worker's instructions, so they cannot contradict each other (the
//! architecture challenge, 5 November, found five copies that did). Both renderings carry the
//! one category clause, [`DECISIONS`], word for word; the full one adds explanation only. Words
//! only: asking changes nothing, and what may run is enforced where it runs (the node, the
//! actuator), never here.

/// The decisions that are the owner's: the one category clause both renderings carry. "Taste
/// the owner kept" is the owner's reserved personal preference (it was "taste" in HQ's tool).
pub const DECISIONS: &str = "money past allowance, accounts, credentials, irreversible or \
     data-losing steps, accepting risk, taste the owner kept, leaving scope";

/// What the full rendering explains about the reserved taste.
const TASTE: &str = "Taste the owner kept means a personal preference they keep for \
     themselves, not a choice they have already given you.";

/// What a question carries.
const ASK_HOW: &str = "Say what you observed, the evidence, the step you propose and the one \
     decision you need from the owner.";

/// An approval requirement is a limit, not a reason.
const ASK_LIMIT: &str = "That a step would need the owner's approval limits what you may do \
     alone; it is not a reason to propose the step: propose one only when a problem or an \
     improvement you can show calls for it.";

/// Asking is a request only.
const ASK_ONLY: &str = "Asking requests a decision and changes nothing; what may run is still \
     enforced where it runs.";

/// The rest is the member's.
const DECIDE_REST: &str = "Within your permissions, decide the rest yourself.";

/// The one statement of when to ask the owner, whole, as the worker's instructions carry it.
#[must_use]
pub fn when_to_ask() -> String {
    format!(
        "Ask the owner only for a decision your own authority does not cover: {DECISIONS}. That \
         holds for a need your task names and for one you discover yourself. {TASTE} {ASK_HOW} \
         {ASK_LIMIT} {ASK_ONLY} {DECIDE_REST}"
    )
}

/// The same statement, brief, where a tool's description must stay short (HQ's `ask_owner`,
/// within its 650-byte budget): the same category clause, then found-or-named and the limit.
#[must_use]
pub fn when_to_ask_briefly() -> String {
    format!("only on {DECISIONS}, found or named; approval rules limit, never motivate.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_statement_covers_the_owners_decisions_found_or_named_and_stays_guidance() {
        let said = when_to_ask();
        for part in [
            "money past allowance",
            "accounts, credentials",
            "irreversible or data-losing steps",
            "accepting risk",
            "taste the owner kept",
            "not a choice they have already given you",
            "leaving scope",
            "one you discover yourself",
            "what you observed, the evidence, the step you propose and the one decision",
            "it is not a reason to propose the step",
            "changes nothing",
            "enforced where it runs",
        ] {
            assert!(said.contains(part), "{part:?} missing from: {said}");
        }
        assert!(!said.contains("Anything else"), "{said}");
    }

    /// Both renderings carry the one category clause word for word: no second list to drift.
    #[test]
    fn both_renderings_carry_the_one_category_clause() {
        assert!(when_to_ask().contains(DECISIONS));
        let brief = when_to_ask_briefly();
        assert!(brief.contains(DECISIONS), "{brief}");
        assert!(brief.contains("found or named"), "{brief}");
        assert!(brief.contains("never motivate"), "{brief}");
    }
}
