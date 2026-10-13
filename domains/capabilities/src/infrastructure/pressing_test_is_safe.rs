use super::*;

/// A skill that can change the world, and records whether it was run.
struct Actuator(std::sync::atomic::AtomicBool);
impl Capability for Actuator {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "wire_transfer",
            name: "Wire transfer",
            description: "",
            category: "",
            reaches_external: true,
            reversibility: Reversibility::Irreversible,
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
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Value::Null)
    }
}

/// A read-only skill, which proves itself by running.
struct Reader;
impl Capability for Reader {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "weather",
            name: "Weather",
            description: "",
            category: "",
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
        Ok(serde_json::json!({ "temp": 72 }))
    }
    fn summarize(&self, _o: &Value) -> String {
        "72F, sunny".to_owned()
    }
}

/// A read-only skill that cannot answer without being asked something.
struct NeedsAQuery;
impl Capability for NeedsAQuery {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "web_search",
            name: "Internet search",
            description: "",
            category: "",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            borrows_from: None,
            third_party: true,
            input_schema: Some(
                r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#,
            ),
        }
    }
    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let query = str_field(input, "query")?;
        Ok(serde_json::json!({ "searched": query }))
    }
    fn summarize(&self, o: &Value) -> String {
        format!("searched for {}", o["searched"])
    }
}

#[test]
fn a_skill_that_needs_a_query_is_tested_with_one() {
    // Live: the Test button on a configured search key answered "missing 'query'", which
    // read as a failure of the key. The key was fine; the test asked it nothing.
    assert_eq!(
        NeedsAQuery
            .self_test(&CapabilitySettings::default())
            .unwrap(),
        "searched for \"test\""
    );
}

#[test]
fn a_skill_that_can_act_is_never_run_to_test_it() {
    // "Press this to find out" must never be how someone discovers what a skill does.
    // The wire transfer is the deliberately absurd case, and the rule is the same for
    // every actuator: turning a light on to prove the light skill works is still an
    // action nobody asked for.
    let actuator = Actuator(std::sync::atomic::AtomicBool::new(false));
    let answer = actuator.self_test(&CapabilitySettings::default());

    assert!(answer.is_err(), "an actuator must refuse: {answer:?}");
    assert!(
        !actuator.0.load(std::sync::atomic::Ordering::SeqCst),
        "it RAN — a test button that transfers money is worse than no test button"
    );
}

#[test]
fn a_read_only_skill_proves_itself_by_running() {
    // The default is not a stub: every observing skill gets a working test without
    // anyone writing one.
    assert_eq!(
        Reader.self_test(&CapabilitySettings::default()).unwrap(),
        "72F, sunny"
    );
}

#[test]
fn a_skill_that_cannot_reach_its_service_says_so_rather_than_claiming_to_work() {
    // Home Assistant with no URL configured — the exact case the button exists for.
    let answer = HomeAssistantCapability.self_test(&CapabilitySettings::default());
    let Err(CapabilityError::Unavailable(why)) = answer else {
        panic!("expected an unavailable answer, got {answer:?}");
    };
    assert!(why.contains("URL"), "{why}");
}
