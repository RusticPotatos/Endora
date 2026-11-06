//! When to ask the owner (climb 2, S3): one statement in one place, rendered into HQ's
//! `ask_owner` tool and the worker's instructions, so they cannot contradict each other (the
//! architecture challenge, 5 November, found five copies that did). Words only: asking changes
//! nothing, and what may run is enforced where it runs (the node, the actuator), never here.

/// Which decisions are the owner's: what the member's own authority does not cover, whether
/// the task names the need or the member finds it.
pub const ASK_WHEN: &str = "Ask the owner only for a decision your own authority does not cover: \
     spending money beyond what you are allowed, changing an account or a credential, an \
     irreversible or data-losing step, accepting a risk, or acting outside the scope you were \
     given. That holds for a need your task names and for one you discover yourself.";

/// What a question carries.
pub const ASK_HOW: &str = "Say what you observed, the evidence, the step you propose and the one \
     decision you need from the owner.";

/// An approval requirement is a limit, not a reason.
pub const ASK_LIMIT: &str = "That a step would need the owner's approval limits what you may do \
     alone; it is not a reason to propose the step: propose one only when a problem or an \
     improvement you can show calls for it.";

/// Asking is a request only.
pub const ASK_ONLY: &str = "Asking requests a decision and changes nothing; what may run is still \
     enforced where it runs.";

/// The rest is the member's.
pub const DECIDE_REST: &str = "Within your permissions, decide the rest yourself.";

/// The one statement of when to ask the owner, as every surface says it.
#[must_use]
pub fn when_to_ask() -> String {
    [ASK_WHEN, ASK_HOW, ASK_LIMIT, ASK_ONLY, DECIDE_REST].join(" ")
}

/// The same statement, brief, where a tool's description must stay short (HQ's `ask_owner`).
#[must_use]
pub fn when_to_ask_briefly() -> &'static str {
    "only what exceeds your authority (money, accounts, credentials, irreversible steps, \
     accepting risk, leaving scope), found or named; approval rules limit, never motivate."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_statement_covers_the_owners_decisions_found_or_named_and_stays_guidance() {
        let said = when_to_ask();
        for part in [
            "spending money",
            "account or a credential",
            "irreversible or data-losing",
            "accepting a risk",
            "outside the scope you were given",
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

    /// The brief rendering names the same decisions as the whole statement, found or named.
    #[test]
    fn the_brief_rendering_names_the_same_decisions() {
        let brief = when_to_ask_briefly();
        for part in [
            "money",
            "accounts",
            "credentials",
            "irreversible",
            "accepting risk",
            "leaving scope",
            "found or named",
            "never motivate",
        ] {
            assert!(brief.contains(part), "{part:?} missing from: {brief}");
        }
        assert!(brief.len() <= 180, "{} bytes", brief.len());
    }
}
