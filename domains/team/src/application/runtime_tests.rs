//! What runs a member's turns (ADR 0117): the owner's setting, refused unless allowed, the
//! worker again when it no longer is, and the pilot seeded once.

use super::engine::{Engine, EngineError, Settings};
use super::ports::{Clock, TeamStore};
use crate::domain::{Refusal, Role, Runtime};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Noon;
impl Clock for Noon {
    fn now(&self) -> i64 {
        1_792_166_400_000
    }
}

fn engine() -> Engine<SqliteTeamStore, SimulatedRunner, Noon> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        Noon,
        Settings::default(),
    )
}

#[test]
fn claude_code_is_set_for_a_developer_on_claude_and_refused_for_anyone_else() {
    let hq = engine();
    let dev = hq.hire("Jane Doe", Role::Developer, 40, 2, 0).unwrap();
    let lead = hq.hire("John Doe", Role::Lead, 25, 2, 1).unwrap();
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::Worker, "the default");
    assert!(
        matches!(
            hq.set_runtime(dev, "claude-code"),
            Err(EngineError::Refused(Refusal::NotARuntime { .. }))
        ),
        "a developer on the house's model"
    );
    hq.set_brain(dev, "claude-haiku-5-5").unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    assert_eq!(
        hq.set_runtime(dev, "claude-code").unwrap(),
        Runtime::ClaudeCode
    );
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::ClaudeCode);
    assert!(
        matches!(
            hq.set_runtime(lead, "claude-code"),
            Err(EngineError::Refused(Refusal::NotARuntime { .. }))
        ),
        "a lead keeps Endora's worker"
    );
    assert!(hq.set_runtime(dev, "codex").is_err());
    let said = hq.events_after(0).unwrap();
    assert!(
        said.iter()
            .any(|e| e.kind == "member.runtime" && e.data["runtime"] == "claude-code"),
        "said in the feed"
    );
    // Its brain sent home, the member is on the worker again, whatever the setting says.
    hq.set_brain(dev, "local").unwrap();
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::Worker);
    hq.set_brain(dev, "claude-sonnet-5-5").unwrap();
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::ClaudeCode);
    assert_eq!(hq.set_runtime(dev, "worker").unwrap(), Runtime::Worker);
}

#[test]
fn the_pilot_puts_rae_on_claude_code_once_and_zed_stays_on_the_worker() {
    let hq = engine();
    assert_eq!(hq.seed_claude_code_pilot().unwrap(), None, "no Rae yet");
    hq.seat_ceo("Mara").unwrap();
    hq.seed_wing().unwrap().expect("the wing is seeded");
    let named = |name: &str| {
        hq.store()
            .members()
            .unwrap()
            .into_iter()
            .find(|m| m.name == name)
            .unwrap()
            .id
    };
    let (rae, zed, kit) = (named("Rae"), named("Zed"), named("Kit"));
    assert_eq!(hq.seed_claude_code_pilot().unwrap(), Some(rae));
    assert_eq!(hq.runtime_of(rae).unwrap(), Runtime::ClaudeCode);
    assert_eq!(hq.runtime_of(zed).unwrap(), Runtime::Worker);
    assert_eq!(hq.runtime_of(kit).unwrap(), Runtime::Worker);
    // The owner's later choice stands: seeding again changes nothing.
    hq.set_runtime(rae, "worker").unwrap();
    assert_eq!(hq.seed_claude_code_pilot().unwrap(), None);
    assert_eq!(hq.runtime_of(rae).unwrap(), Runtime::Worker);
}
