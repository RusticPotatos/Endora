//! Registration for native channels (ADR 0050/0054).
//!
//! The node used to build the one existing channel by name, in the composition root, with
//! an early return when its settings were missing — so a second local integration could
//! not have been reached even after somebody wrote it. These are the tests that could not
//! be written against that shape.

use super::{
    Capability, CapabilityError, CapabilityInfo, CapabilitySettings, NativeChannel, channels_of,
};
use crate::domain::TargetAlias;
use endora_kernel::Reversibility;
use serde_json::Value;
use std::sync::Arc;

/// A channel that remembers which names it was told about, so the test can see that the
/// right aliases reached the right integration.
struct Speaks(Vec<String>);
impl NativeChannel for Speaks {
    fn known(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self.0.iter().map(|n| (n.clone(), n.clone())).collect())
    }
    fn reading(&self) -> Result<String, String> {
        Ok(String::new())
    }
    fn act(&self, _tool: &str, _id: &str) -> Option<Result<String, String>> {
        None
    }
}

/// A skill that speaks for a service, once it has been configured.
struct Local {
    id: &'static str,
    server: &'static str,
}

impl Capability for Local {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: self.id,
            name: "a local thing",
            description: "",
            category: "presence",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            borrows_from: None,
            third_party: false,
            input_schema: None,
        }
    }
    fn invoke(
        &self,
        _input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        Ok(Value::Null)
    }
    fn channel(
        &self,
        settings: &CapabilitySettings,
        aliases: &[TargetAlias],
    ) -> Option<(String, Arc<dyn NativeChannel>)> {
        // Not configured is the integration's own judgement, not the caller's.
        settings.get("url")?;
        let mine = aliases
            .iter()
            .filter(|a| a.server == self.server)
            .map(|a| a.said.clone())
            .collect();
        Some((self.server.to_owned(), Arc::new(Speaks(mine))))
    }
}

/// A perfectly ordinary skill: it answers questions and watches nothing.
struct JustAnswers;
impl Capability for JustAnswers {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "just_answers",
            name: "answers",
            description: "",
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            borrows_from: None,
            third_party: false,
            input_schema: None,
        }
    }
    fn invoke(
        &self,
        _input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        Ok(Value::Null)
    }
}

fn configured(id: &str) -> (String, CapabilitySettings) {
    let mut s = CapabilitySettings::default();
    s.insert("url".to_owned(), "http://x".to_owned());
    (id.to_owned(), s)
}

#[test]
fn a_second_local_integration_is_actually_reached() {
    // The regression this refactor exists for. Under the old shape the first unconfigured
    // integration returned an empty list for everybody, so this could not have passed —
    // and nobody would have found out until they wrote the second one.
    let skills: Vec<Arc<dyn Capability>> = vec![
        Arc::new(Local {
            id: "one",
            server: "server-one",
        }),
        Arc::new(JustAnswers),
        Arc::new(Local {
            id: "two",
            server: "server-two",
        }),
    ];
    let settings = [configured("one"), configured("two")].into_iter().collect();

    let found: Vec<String> = channels_of(&skills, &settings, &[])
        .into_iter()
        .map(|(server, _)| server)
        .collect();
    assert_eq!(found, vec!["server-one", "server-two"]);
}

#[test]
fn one_unconfigured_integration_does_not_silence_the_others() {
    // Precisely the old bug: `settings.get(..) else { return Vec::new() }` meant the first
    // miss ended registration for everything behind it.
    let skills: Vec<Arc<dyn Capability>> = vec![
        Arc::new(Local {
            id: "not_set_up",
            server: "quiet",
        }),
        Arc::new(Local {
            id: "ready",
            server: "loud",
        }),
    ];
    let settings = [configured("ready")].into_iter().collect();

    let found: Vec<String> = channels_of(&skills, &settings, &[])
        .into_iter()
        .map(|(server, _)| server)
        .collect();
    assert_eq!(found, vec!["loud"]);
}

#[test]
fn a_skill_that_only_answers_questions_supplies_nothing() {
    // The default, and the ordinary case: almost every skill wants nothing to do with this.
    let skills: Vec<Arc<dyn Capability>> = vec![Arc::new(JustAnswers)];
    assert!(channels_of(&skills, &std::collections::HashMap::new(), &[]).is_empty());
}

#[test]
fn each_integration_is_handed_every_alias_and_keeps_its_own() {
    // Filtering belongs to the integration: which confirmed names are its own is knowledge
    // only it has, and deciding out here would put a per-integration branch back into
    // shared registration code (ADR 0054).
    let skills: Vec<Arc<dyn Capability>> = vec![
        Arc::new(Local {
            id: "one",
            server: "server-one",
        }),
        Arc::new(Local {
            id: "two",
            server: "server-two",
        }),
    ];
    let settings = [configured("one"), configured("two")].into_iter().collect();
    let aliases = vec![
        TargetAlias::new("server-one", "the lamp", "Living Room Lamp").unwrap(),
        TargetAlias::new("server-two", "the panel", "Alarm").unwrap(),
    ];

    let channels = channels_of(&skills, &settings, &aliases);
    let names_of = |i: usize| -> Vec<String> {
        channels[i]
            .1
            .known()
            .unwrap()
            .into_iter()
            .map(|(said, _)| said)
            .collect()
    };
    assert_eq!(names_of(0), vec!["the lamp"]);
    assert_eq!(names_of(1), vec!["the panel"]);
}
