//! What runs a member's turn (ADR 0117): Endora's own worker, or Claude Code, headless, in the
//! same workbench. The worker is every member's unless the owner says otherwise; Claude Code
//! is offered only to a developer who thinks with Claude, since it is a coding agent and speaks
//! only Anthropic's API (through the node's relay, with the turn's token, never a subscription
//! login). A member set to Claude Code whose brain later goes home runs on the worker again:
//! [`Runtime::effective`] decides at each turn, so a stale setting never sends a turn out.

use super::brain::Brain;
use super::work::Refusal;

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

    /// Whether a member thinking with `brain` may run on this: the worker always; Claude Code
    /// only on Claude, the brains that speak Anthropic's API, as the runner checks (not every
    /// cloud brain: DeepSeek is not one).
    #[must_use]
    pub const fn allowed_for(self, brain: Brain) -> bool {
        match self {
            Self::Worker => true,
            Self::ClaudeCode => brain.claude(),
        }
    }

    /// The runtime the owner asked for by `name`, for a member on `brain`.
    ///
    /// # Errors
    /// [`Refusal::NotARuntime`] for a name that is not one, or Claude Code for a member that
    /// does not think with Claude.
    pub fn for_member(brain: Brain, name: &str) -> Result<Self, Refusal> {
        let refused = || Refusal::NotARuntime {
            asked: name.chars().take(60).collect(),
        };
        let runtime = Self::from_name(name.trim()).ok_or_else(refused)?;
        if !runtime.allowed_for(brain) {
            return Err(refused());
        }
        Ok(runtime)
    }

    /// What runs the turn now: the setting while it is still allowed, the worker otherwise.
    #[must_use]
    pub const fn effective(self, brain: Brain) -> Self {
        if self.allowed_for(brain) {
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
    fn claude_code_is_for_a_member_on_claude_alone() {
        for brain in [Brain::Haiku, Brain::Sonnet] {
            assert_eq!(
                Runtime::for_member(brain, "claude-code"),
                Ok(Runtime::ClaudeCode)
            );
            assert_eq!(Runtime::ClaudeCode.effective(brain), Runtime::ClaudeCode);
        }
        for brain in [Brain::Local, Brain::Mac] {
            assert!(matches!(
                Runtime::for_member(brain, "claude-code"),
                Err(Refusal::NotARuntime { .. })
            ));
            assert_eq!(Runtime::for_member(brain, "worker"), Ok(Runtime::Worker));
            assert_eq!(Runtime::ClaudeCode.effective(brain), Runtime::Worker);
        }
        assert!(Runtime::for_member(Brain::Haiku, "bash").is_err());
    }

    #[test]
    fn claude_code_runs_only_claude_not_every_cloud_model() {
        // Round 2 (31 Oct): DeepSeek is a cloud model but not Claude; Claude Code speaks only
        // Anthropic's API, so the runner refused every turn of a developer moved to it.
        assert!(!Runtime::ClaudeCode.allowed_for(Brain::DeepSeek));
        assert!(Runtime::for_member(Brain::DeepSeek, "claude-code").is_err());
        assert_eq!(
            Runtime::ClaudeCode.effective(Brain::DeepSeek),
            Runtime::Worker
        );
        for brain in [Brain::Opus, Brain::Sonnet, Brain::Haiku] {
            assert!(Runtime::ClaudeCode.allowed_for(brain), "{brain:?}");
        }
    }
}
