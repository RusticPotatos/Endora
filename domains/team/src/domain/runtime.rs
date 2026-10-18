//! What runs a member's turn (ADR 0117): Endora's own worker, or Claude Code, headless, in the
//! same workbench. The worker is every member's unless the owner says otherwise; Claude Code
//! is offered only to a developer who thinks with Claude, since it is a coding agent and speaks
//! only Anthropic's API (through the node's relay, with the turn's token, never a subscription
//! login). A member set to Claude Code whose brain later goes home runs on the worker again:
//! [`Runtime::effective`] decides at each turn, so a stale setting never sends a turn out.

use super::brain::Brain;
use super::work::{Refusal, Role};

/// What runs a member's turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Runtime {
    /// Endora's own worker: its loop, its tools, its rounds.
    #[default]
    Worker,
    /// Claude Code, headless (`claude -p`), started by the worker in the workbench.
    ClaudeCode,
}

/// Every runtime, in the order the owner is offered them.
const ALL: [Runtime; 2] = [Runtime::Worker, Runtime::ClaudeCode];

impl Runtime {
    /// Its stable name, for storage, the runner and the relay: `worker` or `claude-code`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::ClaudeCode => "claude-code",
        }
    }

    /// What the owner reads on the member's card.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Worker => "Endora's worker",
            Self::ClaudeCode => "Claude Code",
        }
    }

    /// The runtime with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        ALL.into_iter().find(|r| r.name() == name)
    }

    /// Whether a member in `role` thinking with `brain` may run on this: the worker always;
    /// Claude Code only for a developer on Claude.
    #[must_use]
    pub const fn allowed_for(self, role: Role, brain: Brain) -> bool {
        match self {
            Self::Worker => true,
            Self::ClaudeCode => matches!(role, Role::Developer) && brain.cloud(),
        }
    }

    /// The runtime the owner asked for by `name`, for a member in `role` on `brain`.
    ///
    /// # Errors
    /// [`Refusal::NotARuntime`] for a name that is not one, or Claude Code for anyone but a
    /// developer on Claude.
    pub fn for_member(role: Role, brain: Brain, name: &str) -> Result<Self, Refusal> {
        let refused = || Refusal::NotARuntime {
            role,
            asked: name.chars().take(60).collect(),
        };
        let runtime = Self::from_name(name.trim()).ok_or_else(refused)?;
        if !runtime.allowed_for(role, brain) {
            return Err(refused());
        }
        Ok(runtime)
    }

    /// What runs the turn now: the setting while it is still allowed, the worker otherwise.
    #[must_use]
    pub const fn effective(self, role: Role, brain: Brain) -> Self {
        if self.allowed_for(role, brain) {
            return self;
        }
        Self::Worker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_member_runs_on_the_worker_unless_the_owner_says_otherwise() {
        assert_eq!(Runtime::default(), Runtime::Worker);
        for runtime in ALL {
            assert_eq!(Runtime::from_name(runtime.name()), Some(runtime));
        }
        assert_eq!(Runtime::from_name("claude"), None);
    }

    #[test]
    fn claude_code_is_a_developer_on_claudes_alone() {
        for brain in [Brain::Haiku, Brain::Sonnet] {
            assert_eq!(
                Runtime::for_member(Role::Developer, brain, "claude-code"),
                Ok(Runtime::ClaudeCode)
            );
        }
        for (role, brain) in [
            (Role::Developer, Brain::Local),
            (Role::Lead, Brain::Sonnet),
            (Role::Ceo, Brain::Opus),
            (Role::Reviewer, Brain::Haiku),
            (Role::Researcher, Brain::Haiku),
        ] {
            assert!(
                matches!(
                    Runtime::for_member(role, brain, "claude-code"),
                    Err(Refusal::NotARuntime { .. })
                ),
                "{role:?} on {brain:?}"
            );
            assert_eq!(
                Runtime::for_member(role, brain, "worker"),
                Ok(Runtime::Worker)
            );
        }
        assert!(Runtime::for_member(Role::Developer, Brain::Haiku, "bash").is_err());
        let said = Runtime::for_member(Role::Lead, Brain::Sonnet, "claude-code")
            .unwrap_err()
            .to_string();
        assert!(said.contains("developer"), "{said}");
    }

    #[test]
    fn a_setting_no_longer_allowed_falls_back_to_the_worker() {
        assert_eq!(
            Runtime::ClaudeCode.effective(Role::Developer, Brain::Haiku),
            Runtime::ClaudeCode
        );
        assert_eq!(
            Runtime::ClaudeCode.effective(Role::Developer, Brain::Local),
            Runtime::Worker,
            "a brain gone home takes the worker again"
        );
        assert_eq!(
            Runtime::ClaudeCode.effective(Role::Lead, Brain::Sonnet),
            Runtime::Worker
        );
    }
}
