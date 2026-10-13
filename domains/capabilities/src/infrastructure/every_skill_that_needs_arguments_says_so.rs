//! The gap that bit twice in one afternoon, and the second time was worse.
//!
//! A skill that requires an argument and declares no schema is offered to the model as
//! taking nothing. The venue skill was called with `{}` and answered "say what to look
//! for", twice — visibly broken, and recoverable. `web_fetch` was called with `{}` too,
//! and the model supplied `https://example.com`, which **succeeded**: it came back with
//! "this domain is for use in documentation examples" and the butler answered from it.
//!
//! Fixing the one in front of me and not sweeping for the rest is what let the second
//! one ship. This sweeps.

use crate::{CapabilityError, CapabilitySettings};

/// Called with no arguments and no settings, so nothing here reaches a network: a skill
/// that needs either refuses before it would.
#[test]
fn a_skill_that_refuses_an_empty_call_has_declared_its_arguments() {
    let empty = CapabilitySettings::default();
    let mut offenders = Vec::new();
    for c in super::default_capabilities() {
        let info = c.info();
        // Irreversible skills are never invoked to find out what they do.
        if info.reversibility != endora_kernel::Reversibility::Observe {
            continue;
        }
        let refused_for_arguments = matches!(
            c.invoke(&serde_json::json!({}), &empty),
            Err(CapabilityError::BadInput(_))
        );
        if refused_for_arguments && info.input_schema.is_none() {
            offenders.push(info.id);
        }
    }
    assert!(
        offenders.is_empty(),
        "these need arguments and tell the model they need none, so it will guess — \
         and a guess that happens to work is worse than one that fails: {offenders:?}"
    );
}
