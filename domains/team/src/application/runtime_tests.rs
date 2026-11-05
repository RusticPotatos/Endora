//! What runs a member's turns (ADR 0117): the owner's setting, refused unless allowed, the
//! worker again when it no longer is.

use super::engine::{Engine, EngineError, Settings};
use super::ports::{Clock, TeamStore};
use crate::domain::{Refusal, Runtime};
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
fn claude_code_is_set_for_a_member_on_claude_and_refused_on_the_house() {
    let hq = engine();
    let dev = hq.hire("Jane Doe", 40, 2, 0).unwrap();
    let lead = hq.hire("John Doe", 25, 2, 1).unwrap();
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::Worker, "the default");
    // A developer on Claude may be set to Claude Code.
    hq.set_brain(dev, "claude-haiku-5-5").unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    assert_eq!(
        hq.set_runtime(dev, "claude-code").unwrap(),
        Runtime::ClaudeCode
    );
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::ClaudeCode);
    assert_eq!(
        hq.set_runtime(lead, "claude-code").unwrap(),
        Runtime::ClaudeCode,
        "a title is a name only"
    );
    hq.set_brain(lead, "local").unwrap();
    assert!(
        matches!(
            hq.set_runtime(lead, "claude-code"),
            Err(EngineError::Refused(Refusal::NotARuntime { .. }))
        ),
        "the house's model keeps Endora's worker"
    );
    assert!(hq.set_runtime(dev, "codex").is_err());
    let said = hq.events_after(0).unwrap();
    assert!(
        said.iter()
            .any(|e| e.kind == "member.runtime" && e.data["runtime"] == "claude-code"),
        "said in the feed"
    );
    // On the house's model it runs on the worker, and the setting stands for when it is back
    // on Claude.
    hq.set_brain(dev, "local").unwrap();
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::Worker);
    hq.set_brain(dev, "claude-sonnet-5-5").unwrap();
    assert_eq!(hq.runtime_of(dev).unwrap(), Runtime::ClaudeCode);
    assert_eq!(hq.set_runtime(dev, "worker").unwrap(), Runtime::Worker);
}

#[test]
fn a_model_claude_code_cannot_run_puts_the_member_on_the_worker_and_says_so() {
    // Round 2 (31 Oct): Rae moved to DeepSeek kept Claude Code, and the runner refused every
    // turn of hers for three hours.
    let hq = engine();
    let rae = hq.hire("Rae", 40, 2, 0).unwrap();
    hq.set_brain(rae, "claude-sonnet-5-5").unwrap();
    hq.set_runtime(rae, "claude-code").unwrap();
    let set = hq.set_model(rae, "deepseek-flash").unwrap();
    assert_eq!(set.brain, crate::domain::Brain::DeepSeek);
    assert_eq!(
        set.said.as_deref(),
        Some("Rae now runs on Endora's worker: Claude Code runs only Claude")
    );
    assert_eq!(hq.runtime_of(rae).unwrap(), Runtime::Worker);
    assert_eq!(
        hq.store().runtime_of(rae).unwrap(),
        Runtime::Worker,
        "the setting itself moved, so the console shows what runs"
    );
    let said = hq.events_after(0).unwrap();
    assert!(
        said.iter()
            .any(|e| e.kind == "member.runtime" && e.data["runtime"] == "worker"),
        "said in the feed"
    );
    // Claude Code is refused while it cannot run her model.
    assert!(matches!(
        hq.set_runtime(rae, "claude-code"),
        Err(EngineError::Refused(Refusal::NotARuntime { .. }))
    ));
    // Another Claude keeps Claude Code, and nothing more is said.
    hq.set_model(rae, "claude-haiku-5-5").unwrap();
    hq.set_runtime(rae, "claude-code").unwrap();
    let set = hq.set_model(rae, "claude-sonnet-5-5").unwrap();
    assert_eq!(set.said, None);
    assert_eq!(hq.runtime_of(rae).unwrap(), Runtime::ClaudeCode);
    // A member on the worker is told nothing whatever the model.
    hq.set_runtime(rae, "worker").unwrap();
    assert_eq!(hq.set_model(rae, "mac").unwrap().said, None);
}
