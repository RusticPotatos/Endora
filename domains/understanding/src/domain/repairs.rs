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

/// What would actually fix a finding — which is a different question from what went
/// wrong, and the only one the person can answer (ADR 0040).
///
/// The two are told apart by **how wide the failure is**, which needs no knowledge of
/// any particular server:
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remedy {
    /// The tool works, but not on this. Something else it was aimed at succeeded, so
    /// what it was called is the likely problem — ask what the thing is really named.
    NameTheTarget,
    /// The tool has never worked on anything. No name fixes that; the useful answer is
    /// to stop offering it, so the model reaches for something that does work.
    StopOfferingIt,
}

/// How many **different** targets a capability must have failed on before the tool
/// itself is the more likely explanation than any one name (ADR 0040). Two, for the same
/// reason as above: one target failing repeatedly is exactly the alias case, and it must
/// keep deriving the alias question rather than being escalated into a withdrawal.
const ENOUGH_TARGETS_TO_BLAME_THE_TOOL: usize = 2;

/// A capability that keeps reporting success while changing nothing (ADR 0039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairProposal {
    /// The capability that keeps not working.
    pub capability: String,
    /// What its calls were aimed at, in the person's own words where they gave any —
    /// the target values from the action's arguments, not the whole input. **Empty**
    /// when the finding is about the capability itself rather than one of its targets.
    pub target: String,
    /// How many times it reported success and changed nothing.
    pub attempts: usize,
    /// What would fix it, derived from how wide the failure is.
    pub remedy: Remedy,
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
    // The two streams a withdrawal needs, kept apart from the counts above because a
    // withdrawal is a much stronger claim and cannot rest on the weaker evidence.
    let mut refusals: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    let mut ever_worked: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for outcome in outcomes {
        if !failed_outright(outcome) {
            ever_worked.insert(outcome.capability().to_owned());
        }
        if !is_not_working(outcome) {
            continue;
        }
        let key = (outcome.capability().to_owned(), target_of(outcome.input()));
        *counts.entry(key.clone()).or_default() += 1;
        if failed_outright(outcome) {
            *refusals.entry(key).or_default() += 1;
        }
    }
    // A capability whose failures span several different targets and which has never
    // once worked is not being mis-aimed — it is the wrong tool (ADR 0040). One finding
    // stands for all of its targets, and its per-target findings are suppressed so the
    // person is asked one question rather than the same question per noun.
    let mut wholly_broken: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for (capability, targets) in group_by_capability(&refusals) {
        if ever_worked.contains(&capability) {
            continue;
        }
        // Width is counted over targets that are each already a pattern. A target that
        // failed once is noise everywhere else in this derivation and must not become
        // evidence about the tool by being counted here — otherwise one bad attempt at
        // the garage escalates a plain alias question into "stop offering this".
        let repeatedly: Vec<&(String, usize)> = targets
            .iter()
            .filter(|(_, n)| *n >= ENOUGH_TO_BE_A_PATTERN)
            .collect();
        if repeatedly.len() < ENOUGH_TARGETS_TO_BLAME_THE_TOOL {
            continue;
        }
        wholly_broken.insert(capability, targets.iter().map(|(_, n)| *n).sum());
    }
    let mut proposals: Vec<RepairProposal> = counts
        .into_iter()
        // A call that named nothing at all cannot be answered with a name. Asking "what
        // is '' actually called?" is unanswerable, and the calls behind it are the
        // model's own failure to say what it meant — which the runner already refuses.
        .filter(|((_, target), _)| !target.is_empty())
        .filter(|((capability, _), attempts)| {
            *attempts >= ENOUGH_TO_BE_A_PATTERN && !wholly_broken.contains_key(capability)
        })
        .map(|((capability, target), attempts)| RepairProposal {
            capability,
            target,
            attempts,
            remedy: Remedy::NameTheTarget,
        })
        .collect();
    proposals.extend(
        wholly_broken
            .into_iter()
            .map(|(capability, attempts)| RepairProposal {
                capability,
                target: String::new(),
                attempts,
                remedy: Remedy::StopOfferingIt,
            }),
    );
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

/// Regroups the per-target failure counts by capability, so the width of a failure can
/// be read off: how many distinct targets, and how many attempts across all of them.
fn group_by_capability(
    counts: &std::collections::BTreeMap<(String, String), usize>,
) -> std::collections::BTreeMap<String, Vec<(String, usize)>> {
    let mut by_capability: std::collections::BTreeMap<String, Vec<(String, usize)>> =
        std::collections::BTreeMap::new();
    for ((capability, target), attempts) in counts {
        by_capability
            .entry(capability.clone())
            .or_default()
            .push((target.clone(), *attempts));
    }
    by_capability
}

/// Whether the capability itself said it could not do the job — Endora's own `error:`
/// marker on the claim, never any server's error format.
///
/// The **unambiguous** half of the evidence, and the only half a withdrawal may rest on
/// (ADR 0040). "Changed nothing" is ambiguous in ways this is not: switching off an
/// already-off light legitimately changes nothing, and a read-back scoped to the wrong
/// thing reports no change on an action that plainly worked. Live, five `HassTurnOn`
/// calls that really did turn lights on were all recorded `changed: false` — enough,
/// under a rule that counted them, to propose withdrawing the most useful tool in the
/// house. An outright refusal cannot be misread that way.
fn failed_outright(outcome: &Outcome) -> bool {
    outcome.claim().trim_start().starts_with("error:")
}

/// Whether this outcome is evidence the capability is not doing its job: it either
/// failed, or it reported success and demonstrably changed nothing.
///
/// Failure is recognised by Endora's **own** marker on the claim, not by reading any
/// server's error format — the turn records a failed run as `error: …`, so this stays
/// as integration-agnostic as the rest of the derivation.
fn is_not_working(outcome: &Outcome) -> bool {
    outcome.changed() == Some(false) || failed_outright(outcome)
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
    use super::{Remedy, RepairProposal, repair_proposals};
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
                remedy: Remedy::NameTheTarget,
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

    #[test]
    fn a_tool_that_has_never_worked_on_anything_is_the_tool_and_not_the_name() {
        // The live case this exists for. `HassLightSet` sets brightness or colour, so
        // it can never switch a light — and the model kept reaching for it. Attempts at
        // the kitchen table and at the bedroom both failed, and it has never once
        // succeeded. No name fixes that, so asking "what is the table really called?"
        // wastes the person's answer.
        let history = [
            claimed(
                1,
                "home.HassLightSet",
                r#"{"name":"table"}"#,
                "error: no",
                None,
            ),
            claimed(
                2,
                "home.HassLightSet",
                r#"{"name":"table"}"#,
                "error: no",
                None,
            ),
            claimed(
                3,
                "home.HassLightSet",
                r#"{"name":"bedroom"}"#,
                "error: no",
                None,
            ),
            claimed(
                4,
                "home.HassLightSet",
                r#"{"name":"bedroom"}"#,
                "error: no",
                None,
            ),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(
            proposals.len(),
            1,
            "one question, not one per noun: {proposals:?}"
        );
        assert_eq!(proposals[0].remedy, Remedy::StopOfferingIt);
        assert_eq!(proposals[0].attempts, 4, "counts every wasted attempt");
        assert!(
            proposals[0].target.is_empty(),
            "the finding is about the tool, so it names no target: {proposals:?}"
        );
    }

    #[test]
    fn a_tool_that_works_somewhere_is_never_withdrawn() {
        // The distinction that keeps this honest. The same wide failure pattern, plus
        // one success: the tool demonstrably works, so the targets are the problem and
        // the person should be asked about names instead.
        let history = [
            claimed(
                1,
                "home.HassTurnOn",
                r#"{"name":"table"}"#,
                "error: no",
                None,
            ),
            claimed(
                2,
                "home.HassTurnOn",
                r#"{"name":"table"}"#,
                "error: no",
                None,
            ),
            claimed(
                3,
                "home.HassTurnOn",
                r#"{"name":"bedroom"}"#,
                "error: no",
                None,
            ),
            claimed(
                4,
                "home.HassTurnOn",
                r#"{"name":"bedroom"}"#,
                "error: no",
                None,
            ),
            outcome(
                5,
                "home.HassTurnOn",
                r#"{"name":"Garage Main"}"#,
                Some(true),
            ),
        ];
        let proposals = repair_proposals(&history);
        assert!(
            proposals.iter().all(|p| p.remedy == Remedy::NameTheTarget),
            "a tool that works somewhere must not be withdrawn: {proposals:?}"
        );
        assert_eq!(proposals.len(), 2, "{proposals:?}");
    }

    #[test]
    fn an_unverified_success_still_counts_as_the_tool_working() {
        // `changed: None` on a call that did not error means nobody could check, not
        // that it failed (ADR 0038's honest silence). It is not evidence of a problem
        // anywhere else in this derivation, and it must not support a withdrawal here.
        let history = [
            claimed(
                1,
                "cal.CreateEvent",
                r#"{"title":"one"}"#,
                "error: no",
                None,
            ),
            claimed(
                2,
                "cal.CreateEvent",
                r#"{"title":"one"}"#,
                "error: no",
                None,
            ),
            claimed(
                3,
                "cal.CreateEvent",
                r#"{"title":"two"}"#,
                "error: no",
                None,
            ),
            claimed(
                4,
                "cal.CreateEvent",
                r#"{"title":"two"}"#,
                "error: no",
                None,
            ),
            outcome(5, "cal.CreateEvent", r#"{"title":"three"}"#, None),
        ];
        let proposals = repair_proposals(&history);
        assert!(
            proposals.iter().all(|p| p.remedy == Remedy::NameTheTarget),
            "an unverified success is still a success: {proposals:?}"
        );
    }

    #[test]
    fn one_bad_target_does_not_escalate_into_withdrawing_the_tool() {
        // Width is counted over targets that are each already a pattern. Two failures
        // at the kitchen and a single stray one at the garage is the alias case, and
        // must stay the alias question.
        let history = [
            claimed(
                1,
                "home.HassTurnOff",
                r#"{"area":"kitchen"}"#,
                "error: no",
                None,
            ),
            claimed(
                2,
                "home.HassTurnOff",
                r#"{"area":"kitchen"}"#,
                "error: no",
                None,
            ),
            claimed(
                3,
                "home.HassTurnOff",
                r#"{"area":"garage"}"#,
                "error: no",
                None,
            ),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(proposals.len(), 1, "{proposals:?}");
        assert_eq!(proposals[0].remedy, Remedy::NameTheTarget);
        assert_eq!(proposals[0].target, "kitchen");
    }

    #[test]
    fn the_derivation_knows_nothing_about_any_particular_server() {
        // The whole point of ADR 0040: the same finding derives for a calendar, a
        // filesystem, anything — because the rule is about the shape of the history,
        // not about names Endora was taught.
        let history = [
            claimed(1, "files.Move", r#"{"path":"a.txt"}"#, "error: nope", None),
            claimed(2, "files.Move", r#"{"path":"a.txt"}"#, "error: nope", None),
            claimed(3, "files.Move", r#"{"path":"b.txt"}"#, "error: nope", None),
            claimed(4, "files.Move", r#"{"path":"b.txt"}"#, "error: nope", None),
        ];
        let proposals = repair_proposals(&history);
        assert_eq!(proposals.len(), 1, "{proposals:?}");
        assert_eq!(proposals[0].capability, "files.Move");
        assert_eq!(proposals[0].remedy, Remedy::StopOfferingIt);
    }

    #[test]
    fn a_tool_that_really_worked_is_never_withdrawn_over_an_unreliable_no_change_reading() {
        // Straight from production, and it proposed withdrawing the most useful tool in
        // the house. `HassTurnOn` had nine refusals and five calls that DID turn lights
        // on — every one of those recorded `changed: false`, because the read-back was
        // scoped to something else. Counting them as failures made the tool look wholly
        // broken when it plainly works.
        let mut history = vec![
            claimed(
                1,
                "home.HassTurnOn",
                r#"{"name":"table"}"#,
                "error: no match",
                None,
            ),
            claimed(
                2,
                "home.HassTurnOn",
                r#"{"name":"table"}"#,
                "error: no match",
                None,
            ),
            claimed(
                3,
                "home.HassTurnOn",
                r#"{"name":"kitchen"}"#,
                "error: no match",
                None,
            ),
            claimed(
                4,
                "home.HassTurnOn",
                r#"{"name":"kitchen"}"#,
                "error: no match",
                None,
            ),
        ];
        // The successes: no error, and a no-change reading that is simply not to be
        // trusted about whether the light came on.
        history.push(outcome(
            5,
            "home.HassTurnOn",
            r#"{"name":"Garage Main"}"#,
            Some(false),
        ));
        let proposals = repair_proposals(&history);
        assert!(
            proposals.iter().all(|p| p.remedy == Remedy::NameTheTarget),
            "a tool that worked was proposed for withdrawal: {proposals:?}"
        );
    }

    #[test]
    fn withdrawing_rests_only_on_the_tool_refusing_outright() {
        // "Changed nothing" is ambiguous — an already-off light, a mis-scoped read-back.
        // "I cannot do this" is not. Only the second is strong enough to remove a tool,
        // so a history of pure no-ops derives alias questions and never a withdrawal.
        let history = [
            outcome(1, "home.HassTurnOff", r#"{"name":"table"}"#, Some(false)),
            outcome(2, "home.HassTurnOff", r#"{"name":"table"}"#, Some(false)),
            outcome(3, "home.HassTurnOff", r#"{"name":"bedroom"}"#, Some(false)),
            outcome(4, "home.HassTurnOff", r#"{"name":"bedroom"}"#, Some(false)),
        ];
        let proposals = repair_proposals(&history);
        assert!(
            proposals.iter().all(|p| p.remedy == Remedy::NameTheTarget),
            "no-op evidence alone withdrew a tool: {proposals:?}"
        );
        assert_eq!(proposals.len(), 2, "{proposals:?}");
    }

    #[test]
    fn a_call_that_named_nothing_asks_no_question() {
        // Live, and unanswerable: the model sent {area:null, name:null}, so the finding
        // read "attempts aimed at '' didn't work. What is it actually called?" There is
        // no it. The call's failure was saying nothing, which the runner already refuses.
        let history = [
            claimed(
                1,
                "home.HassTurnOn",
                r#"{"area":null,"name":null}"#,
                "error: no",
                None,
            ),
            claimed(
                2,
                "home.HassTurnOn",
                r#"{"area":null,"name":null}"#,
                "error: no",
                None,
            ),
        ];
        assert!(
            repair_proposals(&history).is_empty(),
            "asked the person to name something that was never named"
        );
    }
}
