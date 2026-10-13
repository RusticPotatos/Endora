//! ADR 0060.
//!
//! Live: asked about events in a city, the butler was handed 37 tools — nine skills
//! including one that answers exactly that — and replied that "the functions provided
//! are from Home Assistant". Nothing was unwired.

use super::{Reversibility, offered_and_deferred};
use endora_capabilities::CapabilitySpec;
use std::collections::HashMap;

fn tool(id: &str, band: Reversibility) -> CapabilitySpec {
    CapabilitySpec {
        id: id.to_owned(),
        wants_place: false,
        third_party: false,
        description: String::new(),
        configured: true,
        autonomous: band == Reversibility::Observe,
        input_schema: None,
        reversibility: band,
    }
}

#[test]
fn a_read_is_offered_and_an_actuator_waits() {
    // Whether anyone is home bears on questions with nothing to do with the house;
    // broadcasting through it does not.
    let (offered, deferred) = offered_and_deferred(
        vec![
            tool("home-assistant.HassBroadcast", Reversibility::Irreversible),
            tool("home-assistant.GetLiveContext", Reversibility::Observe),
            tool("news", Reversibility::Observe),
        ],
        &HashMap::new(),
    );
    assert_eq!(
        offered.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["home-assistant.GetLiveContext", "news"]
    );
    assert_eq!(
        deferred.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["home-assistant.HassBroadcast"]
    );
}

#[test]
fn a_read_the_gate_would_refuse_is_not_dangled_in_front() {
    // Live: a stored stance had taken the reader's autonomy, but the offer only
    // looked at the band — so the model was shown a tool the gate refused every
    // single time, called it every single time, and spent the turn apologising.
    // What is offered must be what would actually run; a silenced read waits
    // behind the lookup with everything else the turn cannot simply use.
    let mut silenced = tool("home-assistant.GetLiveContext", Reversibility::Observe);
    silenced.autonomous = false;
    let (offered, deferred) = offered_and_deferred(
        vec![silenced, tool("news", Reversibility::Observe)],
        &HashMap::new(),
    );
    assert_eq!(
        offered.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["news"]
    );
    assert_eq!(
        deferred.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["home-assistant.GetLiveContext"]
    );
}

/// The whole argument for ranking on read-back rather than on a description.
///
/// `HassLightSet` reads perfectly for "turn off the kitchen light" and has never once
/// worked in five attempts. Every published approach ranks by description and would
/// offer it first forever, because none of them ever finds out.
///
/// Amended: a proven actuator is not merely ranked first among the deferred, it is
/// **offered**. Deferring every actuator and trusting the model to open the way back
/// measured 0/3 against the live model.
#[test]
fn a_tool_that_has_worked_is_offered_and_one_that_never_has_is_not() {
    let mut how_it_went = HashMap::new();
    how_it_went.insert("home-assistant.HassLightSet".to_owned(), (0, 5));
    how_it_went.insert("home-assistant.HassTurnOff".to_owned(), (4, 13));
    let (offered, deferred) = offered_and_deferred(
        vec![
            tool("home-assistant.HassLightSet", Reversibility::Reversible),
            tool("home-assistant.HassTurnOff", Reversibility::Reversible),
        ],
        &how_it_went,
    );
    assert_eq!(
        offered.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["home-assistant.HassTurnOff"],
        "a proven actuator has to be reachable without a round-trip"
    );
    assert_eq!(
        deferred.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["home-assistant.HassLightSet"],
        "five attempts and never once worked — it does not belong in front"
    );
}

/// Absence of a decision is not a decision.
///
/// A tool nobody has tried outranks one that has failed every time, and is outranked by
/// one that works. Punishing a new tool for having no record is the same mistake as a
/// standing default re-opening one somebody blocked, pointed the other way.
#[test]
fn a_tool_nothing_is_known_about_sits_between_the_two() {
    let mut how_it_went = HashMap::new();
    how_it_went.insert("never".to_owned(), (0, 4));
    how_it_went.insert("always".to_owned(), (4, 4));
    let (offered, deferred) = offered_and_deferred(
        vec![
            tool("never", Reversibility::Reversible),
            tool("brand-new", Reversibility::Reversible),
            tool("always", Reversibility::Reversible),
        ],
        &how_it_went,
    );
    // Only the proven one comes forward: a tool with no record cannot buy its way to the
    // front, and one that has never worked certainly cannot.
    assert_eq!(
        offered.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["always"]
    );
    // Among what is left, an unknown tool still sits above one that has never worked.
    assert_eq!(
        deferred.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["brand-new", "never"]
    );
}

#[test]
fn with_nothing_recorded_the_order_is_the_one_it_arrived_in() {
    // A sort with nothing to go on must not shuffle. Stability is what keeps a turn
    // reproducible on a fresh install, which is where the eval runs.
    let (_, deferred) = offered_and_deferred(
        vec![
            tool("first", Reversibility::Reversible),
            tool("second", Reversibility::Reversible),
            tool("third", Reversibility::Reversible),
        ],
        &HashMap::new(),
    );
    assert_eq!(
        deferred.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
}
