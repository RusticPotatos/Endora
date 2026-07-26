//! Repair proposals — Endora noticing that its own tooling is wrong (ADR 0039).
//!
//! The observation half of [ADR 0038](../../../docs/adr/0038-capability-profiles.md).
//! Every action leaves an [`Outcome`](crate::domain::Outcome) carrying what the tool
//! claimed and whether the world actually moved; this reads that history back and says
//! when a capability keeps reporting success while changing nothing.
//!
//! Three properties, all deliberate:
//!
//! - **Derived, never stored.** A proposal is computed from outcomes on read. There is
//!   no table, no row to dismiss, nothing to groom — which is how ADR 0029's approval
//!   queue is made impossible rather than merely discouraged. When the outcomes age out,
//!   or an action finally changes something, the proposal stops being derived.
//! - **Deterministic.** Arithmetic over stored records, with no model in the path. The
//!   model is measured at 1 run in 3 on obeying an explicit instruction about
//!   verification; it is not fit to decide what is broken.
//! - **A question, not a repair.** It reports the pattern and asks. It does not parse
//!   the reading to guess an answer — that would mean understanding one server's text
//!   format, which is the per-integration patching ADR 0038 exists to stop.

use crate::domain::Outcome;

/// How many no-change actions on the same target before it is a pattern rather than an
/// accident. Two is the smallest number that can be a repetition; an action legitimately
/// changes nothing often enough (turning off an already-off light) that one is noise.
const ENOUGH_TO_BE_A_PATTERN: usize = 2;

/// A capability that keeps reporting success while changing nothing (ADR 0039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairProposal {
    /// The capability that keeps not working.
    pub capability: String,
    /// What its calls were aimed at, in the person's own words where they gave any —
    /// the target values from the action's arguments, not the whole input.
    pub target: String,
    /// How many times it reported success and changed nothing.
    pub attempts: usize,
}

/// Reads outcome history back and reports capabilities that keep changing nothing.
///
/// Only outcomes Endora actually verified count: `changed == Some(false)` means it read
/// the state before and after and they were identical. A `None` — no reader nominated,
/// or no before-reading — is not evidence of anything and is ignored, the same honest
/// silence ADR 0038 chose for servers nobody has told Endora about.
///
/// Grouped by capability **and target**, because "this tool never works" and "this tool
/// never works *on the kitchen*" are different findings and only the second is
/// actionable.
#[must_use]
pub fn repair_proposals(outcomes: &[Outcome]) -> Vec<RepairProposal> {
    let mut counts: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    for outcome in outcomes {
        if outcome.changed() != Some(false) {
            continue;
        }
        let key = (outcome.capability().to_owned(), target_of(outcome.input()));
        *counts.entry(key).or_default() += 1;
    }
    let mut proposals: Vec<RepairProposal> = counts
        .into_iter()
        .filter(|(_, attempts)| *attempts >= ENOUGH_TO_BE_A_PATTERN)
        .map(|((capability, target), attempts)| RepairProposal {
            capability,
            target,
            attempts,
        })
        .collect();
    // Worst first — the thing that has wasted the most attempts is the thing worth
    // asking about. The key breaks ties so the order is stable.
    proposals.sort_by(|a, b| {
        b.attempts
            .cmp(&a.attempts)
            .then_with(|| a.capability.cmp(&b.capability))
            .then_with(|| a.target.cmp(&b.target))
    });
    proposals
}

/// The human-meaningful part of an action's arguments — the string values, which are
/// what name a thing ("kitchen", "main"). Numbers and flags say *how*, not *what*.
///
/// Deliberately crude and format-agnostic: it never learns a server's argument names, so
/// a calendar's `title` groups exactly as Home Assistant's `area` does. Values are sorted
/// and lowercased so `{area, name}` and `{name, area}` are the same target.
fn target_of(input: &str) -> String {
    let mut values: Vec<String> = input
        .split(['{', '}', ',', ':'])
        .filter_map(|part| {
            let trimmed = part.trim().trim_matches('"').trim();
            (!trimmed.is_empty() && trimmed.chars().any(char::is_alphabetic))
                .then(|| trimmed.to_lowercase())
        })
        .collect();
    values.sort();
    values.dedup();
    values.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{RepairProposal, repair_proposals};
    use crate::domain::Outcome;
    use endora_kernel::ids::{OutcomeId, Timestamp};

    fn outcome(id: u128, capability: &str, input: &str, changed: Option<bool>) -> Outcome {
        Outcome::record(
            OutcomeId::new(id),
            capability,
            input,
            "the action completed successfully",
            Some("kitchen main | switch | on"),
            Timestamp::from_unix_millis(id as i64),
            None,
            changed,
        )
        .expect("valid")
    }

    #[test]
    fn a_capability_that_keeps_changing_nothing_is_reported() {
        // The live case: repeated attempts on the kitchen, every one reporting success,
        // nothing ever moving.
        let history = [
            outcome(1, "home.HassTurnOff", r#"{"area":"kitchen"}"#, Some(false)),
            outcome(2, "home.HassTurnOff", r#"{"area":"kitchen"}"#, Some(false)),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(
            proposals,
            vec![RepairProposal {
                capability: "home.HassTurnOff".to_owned(),
                target: "area kitchen".to_owned(),
                attempts: 2,
            }]
        );
    }

    #[test]
    fn one_no_op_is_an_accident_not_a_finding() {
        // Turning off an already-off light changes nothing and is perfectly correct.
        // Only repetition is evidence.
        let history = [outcome(
            1,
            "home.HassTurnOff",
            r#"{"area":"kitchen"}"#,
            Some(false),
        )];
        assert!(repair_proposals(&history).is_empty());
    }

    #[test]
    fn actions_that_worked_are_not_reported() {
        let history = [
            outcome(1, "home.HassTurnOff", r#"{"area":"kitchen"}"#, Some(true)),
            outcome(2, "home.HassTurnOff", r#"{"area":"kitchen"}"#, Some(true)),
        ];
        assert!(repair_proposals(&history).is_empty());
    }

    #[test]
    fn unverified_actions_are_not_evidence() {
        // No reader nominated means no before/after to compare. Silence is the honest
        // answer, not a guess (ADR 0038).
        let history = [
            outcome(1, "home.HassTurnOff", r#"{"area":"kitchen"}"#, None),
            outcome(2, "home.HassTurnOff", r#"{"area":"kitchen"}"#, None),
        ];
        assert!(repair_proposals(&history).is_empty());
    }

    #[test]
    fn different_targets_are_different_findings() {
        // "this tool never works" and "this tool never works on the kitchen" are not
        // the same thing, and only the second is actionable.
        let history = [
            outcome(1, "home.HassTurnOff", r#"{"area":"kitchen"}"#, Some(false)),
            outcome(2, "home.HassTurnOff", r#"{"area":"kitchen"}"#, Some(false)),
            outcome(3, "home.HassTurnOff", r#"{"area":"garage"}"#, Some(false)),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(proposals.len(), 1, "{proposals:?}");
        assert_eq!(proposals[0].target, "area kitchen");
    }

    #[test]
    fn the_same_target_groups_however_the_model_ordered_it() {
        // A model writes its arguments in whatever order it likes; the same target must
        // not read as two different findings.
        let history = [
            outcome(
                1,
                "home.HassTurnOff",
                r#"{"area":"kitchen","name":"main"}"#,
                Some(false),
            ),
            outcome(
                2,
                "home.HassTurnOff",
                r#"{"name":"main","area":"Kitchen"}"#,
                Some(false),
            ),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(proposals.len(), 1, "{proposals:?}");
        assert_eq!(proposals[0].attempts, 2);
    }

    #[test]
    fn the_worst_offender_comes_first() {
        let history = [
            outcome(1, "a", r#"{"x":"one"}"#, Some(false)),
            outcome(2, "a", r#"{"x":"one"}"#, Some(false)),
            outcome(3, "b", r#"{"x":"two"}"#, Some(false)),
            outcome(4, "b", r#"{"x":"two"}"#, Some(false)),
            outcome(5, "b", r#"{"x":"two"}"#, Some(false)),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(proposals[0].capability, "b");
        assert_eq!(proposals[0].attempts, 3);
    }
}
