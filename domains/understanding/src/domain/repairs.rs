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

/// Reads outcome history back and reports capabilities that keep not working.
///
/// Two shapes count, because they are the same finding from the person's side — this
/// tool does not do what I ask with this target:
///
/// - **Changed nothing.** `changed == Some(false)`: Endora read the state before and
///   after and they were identical, whatever the tool claimed.
/// - **Failed outright.** The tool returned an error. Nine attempts on the kitchen
///   failed to match any entity because the model kept inventing names ("main light",
///   "any kitchen light") for something actually called `Kitchen Main` — and with only
///   the no-change rule, none of that derived a thing.
///
/// An unverified success is still not evidence: `changed == None` on a call that
/// *succeeded* means there was nothing to compare, and silence is the honest answer
/// (ADR 0038).
///
/// Grouped by capability **and target**, because "this tool never works" and "this tool
/// never works *on the kitchen*" are different findings and only the second is
/// actionable.
#[must_use]
pub fn repair_proposals(outcomes: &[Outcome]) -> Vec<RepairProposal> {
    let mut counts: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    for outcome in outcomes {
        if !is_not_working(outcome) {
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

/// Whether this outcome is evidence the capability is not doing its job: it either
/// failed, or it reported success and demonstrably changed nothing.
///
/// Failure is recognised by Endora's **own** marker on the claim, not by reading any
/// server's error format — the turn records a failed run as `error: …`, so this stays
/// as integration-agnostic as the rest of the derivation.
fn is_not_working(outcome: &Outcome) -> bool {
    outcome.changed() == Some(false) || outcome.claim().trim_start().starts_with("error:")
}

/// What an action was aimed at, in words a person would recognise.
///
/// The **values** a call carried, never the field names: `{"area":"kitchen","name":""}`
/// is aimed at `kitchen`, not at `area name kitchen`. Keeping the keys made two calls
/// with the same target read as different findings whenever the model filled a different
/// set of fields — which it does constantly — and produced text nobody could read.
///
/// Array values are skipped for the same reason the read-back ignores them: a scalar
/// points at something, an array restricts which kinds count. `domain: ["light"]` is not
/// part of what was aimed at.
///
/// Deliberately format-agnostic — it never learns a server's argument names, so a
/// calendar's `title` groups exactly as Home Assistant's `area` does.
fn target_of(input: &str) -> String {
    let mut values: Vec<String> = Vec::new();
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        // Find a quoted token.
        if bytes[i] != '"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < bytes.len() && bytes[end] != '"' {
            end += 1;
        }
        let token: String = bytes[start..end].iter().collect();
        // What follows decides whether that token was a key or a value.
        let mut after = end + 1;
        while after < bytes.len() && bytes[after].is_whitespace() {
            after += 1;
        }
        let is_key = after < bytes.len() && bytes[after] == ':';
        if is_key {
            // Step over the value, skipping arrays wholesale.
            let mut v = after + 1;
            while v < bytes.len() && bytes[v].is_whitespace() {
                v += 1;
            }
            if v < bytes.len() && bytes[v] == '[' {
                let mut depth = 0;
                while v < bytes.len() {
                    match bytes[v] {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    v += 1;
                }
                i = v + 1;
                continue;
            }
            i = end + 1;
            continue;
        }
        let trimmed = token.trim();
        if !trimmed.is_empty() && trimmed.chars().any(char::is_alphabetic) {
            values.push(trimmed.to_lowercase());
        }
        i = end + 1;
    }
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
        claimed(
            id,
            capability,
            input,
            "the action completed successfully",
            changed,
        )
    }

    /// An outcome with an explicit claim, so a failure can be expressed.
    fn claimed(
        id: u128,
        capability: &str,
        input: &str,
        claim: &str,
        changed: Option<bool>,
    ) -> Outcome {
        Outcome::record(
            OutcomeId::new(id),
            capability,
            input,
            claim,
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
                target: "kitchen".to_owned(),
                attempts: 2,
            }]
        );
    }

    #[test]
    fn repeated_failures_count_as_much_as_repeated_no_ops() {
        // The live gap: nine attempts on the kitchen FAILED to match anything, because
        // the model kept inventing names for an entity called `Kitchen Main`. Every one
        // had `changed: None` — nothing to compare, since nothing ran — so the
        // no-change rule alone derived nothing at all while the person watched it fail
        // over and over.
        let history = [
            claimed(
                1,
                "home.HassTurnOff",
                r#"{"area":"kitchen","name":"main light"}"#,
                "error: no_match_reason=NAME",
                None,
            ),
            claimed(
                2,
                "home.HassTurnOff",
                r#"{"name":"main light","area":"kitchen"}"#,
                "error: no_match_reason=NAME",
                None,
            ),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(proposals.len(), 1, "{proposals:?}");
        assert_eq!(proposals[0].attempts, 2);
    }

    #[test]
    fn a_success_nobody_could_verify_is_still_not_evidence() {
        // `changed: None` on a call that SUCCEEDED means there was nothing to compare —
        // no reader nominated, or no before-reading — not that something is wrong.
        // Silence is the honest answer, not a guess (ADR 0038).
        let history = [
            outcome(1, "home.HassTurnOff", r#"{"area":"kitchen"}"#, None),
            outcome(2, "home.HassTurnOff", r#"{"area":"kitchen"}"#, None),
        ];
        assert!(repair_proposals(&history).is_empty());
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
        assert_eq!(proposals[0].target, "kitchen");
    }

    #[test]
    fn the_target_reads_as_what_was_aimed_at_not_as_field_names() {
        // Seen live: targets came out as
        //   "[\"light\"] area device_class domain floor kitchen main light name"
        // — field names, array syntax and all. Unreadable, and it split one finding
        // into several whenever the model filled a different set of fields.
        let history = [
            claimed(
                1,
                "c",
                r#"{"floor":"","name":"main light","area":"kitchen","device_class":["light"],"domain":["light"]}"#,
                "error: nope",
                None,
            ),
            claimed(
                2,
                "c",
                r#"{"name":"main light","area":"kitchen"}"#,
                "error: nope",
                None,
            ),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(
            proposals.len(),
            1,
            "the same target split into separate findings: {proposals:?}"
        );
        assert_eq!(proposals[0].target, "kitchen main light");
        assert_eq!(proposals[0].attempts, 2);
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
