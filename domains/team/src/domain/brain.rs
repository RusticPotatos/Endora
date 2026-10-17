//! What a member thinks with (the owner's decisions of 2026-10-17, ADR 0109 and 0111): the
//! house's own model, or Claude through the node's relay, set by the owner per member. Which
//! Claude a role may think with is the plan's models by role (section 9), the same rule the
//! relay enforces on every call: Opus or Sonnet for the CEO; Sonnet or Haiku for the lead, the
//! developer and the reviewer; Haiku only for the researcher. Opus is the CEO's alone.

use super::work::{Refusal, Role};

/// What a member thinks with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Brain {
    /// The house's own model, through the node's local relay: nothing leaves the house, and
    /// it costs nothing.
    #[default]
    Local,
    /// Claude Sonnet, through the node's Anthropic relay, metered against the team's pool.
    Sonnet,
    /// Claude Haiku, through the same relay.
    Haiku,
    /// Claude Opus, through the same relay: the CEO's only (ADR 0111).
    Opus,
}

/// Every brain, in the order the owner is offered them.
const ALL: [Brain; 4] = [Brain::Local, Brain::Opus, Brain::Sonnet, Brain::Haiku];

impl Brain {
    /// Its stable name, for storage, the snapshot and the runner: `local`, or the model id the
    /// worker names to the relay.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Sonnet => "claude-sonnet-5-5",
            Self::Haiku => "claude-haiku-5-5",
            Self::Opus => "claude-opus-5-5",
        }
    }

    /// What the owner reads on the member's card.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Sonnet => "Claude Sonnet",
            Self::Haiku => "Claude Haiku",
            Self::Opus => "Claude Opus",
        }
    }

    /// The brain with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        ALL.into_iter().find(|b| b.name() == name)
    }

    /// Whether what the member reads leaves the house: every brain but the local one. The
    /// node gives a cloud turn no Confidential result beyond what the owner allowed (ADR 0109).
    #[must_use]
    pub const fn cloud(self) -> bool {
        !matches!(self, Self::Local)
    }

    /// Whether a member in `role` may think with this.
    #[must_use]
    pub const fn allowed_for(self, role: Role) -> bool {
        match self {
            Self::Local => true,
            Self::Opus => matches!(role, Role::Ceo),
            Self::Sonnet => !matches!(role, Role::Researcher),
            Self::Haiku => !matches!(role, Role::Ceo),
        }
    }

    /// The brains a member in `role` may be given, in the order the owner is offered them.
    #[must_use]
    pub fn choices(role: Role) -> Vec<Self> {
        ALL.into_iter().filter(|b| b.allowed_for(role)).collect()
    }

    /// The brain the owner asked for by `name`, for a member in `role`.
    ///
    /// # Errors
    /// [`Refusal::NotABrain`] for a name that is not one, or one the role may not use (Opus
    /// for any role but the CEO).
    pub fn for_role(role: Role, name: &str) -> Result<Self, Refusal> {
        let refused = || Refusal::NotABrain {
            role,
            asked: name.chars().take(60).collect(),
        };
        let brain = Self::from_name(name.trim()).ok_or_else(refused)?;
        if !brain.allowed_for(role) {
            return Err(refused());
        }
        Ok(brain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_thinks_locally_unless_the_owner_says_otherwise() {
        assert_eq!(Brain::default(), Brain::Local);
        assert!(!Brain::Local.cloud());
        assert!(Brain::Sonnet.cloud() && Brain::Haiku.cloud() && Brain::Opus.cloud());
        for brain in ALL {
            assert_eq!(Brain::from_name(brain.name()), Some(brain));
        }
    }

    #[test]
    fn the_roles_brains_are_the_plans_models_by_role_and_opus_is_refused() {
        for role in [Role::Lead, Role::Developer, Role::Reviewer] {
            assert_eq!(
                Brain::choices(role),
                [Brain::Local, Brain::Sonnet, Brain::Haiku]
            );
            assert_eq!(
                Brain::for_role(role, "claude-sonnet-5-5"),
                Ok(Brain::Sonnet)
            );
        }
        assert_eq!(
            Brain::choices(Role::Researcher),
            [Brain::Local, Brain::Haiku]
        );
        assert_eq!(
            Brain::for_role(Role::Researcher, "claude-haiku-5-5"),
            Ok(Brain::Haiku)
        );
        // The owner's decision of 2026-10-17 (ADR 0111): the CEO thinks with Opus, Sonnet or
        // the house's model, and Opus is nobody else's.
        assert_eq!(
            Brain::choices(Role::Ceo),
            [Brain::Local, Brain::Opus, Brain::Sonnet]
        );
        assert_eq!(
            Brain::for_role(Role::Ceo, "claude-opus-5-5"),
            Ok(Brain::Opus)
        );
        for (role, asked) in [
            (Role::Researcher, "claude-sonnet-5-5"),
            (Role::Lead, "claude-opus-5-5"),
            (Role::Developer, "claude-opus-5-5"),
            (Role::Reviewer, "claude-opus-5-5"),
            (Role::Researcher, "claude-opus-5-5"),
            (Role::Ceo, "claude-haiku-5-5"),
            (Role::Ceo, "deepseek-chat"),
            (Role::Lead, "gpt-5"),
            (Role::Lead, ""),
            (Role::Developer, "claude-sonnet-5-5-20261001"),
        ] {
            let refused = Brain::for_role(role, asked).unwrap_err();
            assert!(
                matches!(refused, Refusal::NotABrain { .. }),
                "{role:?} {asked}"
            );
        }
        let said = Brain::for_role(Role::Lead, "claude-opus-5-5")
            .unwrap_err()
            .to_string();
        assert!(said.contains("Opus is the CEO's alone"), "{said}");
    }
}
