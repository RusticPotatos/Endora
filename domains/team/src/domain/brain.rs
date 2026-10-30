//! What a member thinks with (the owner's decisions of 2026-10-17, ADR 0109 and 0111): the
//! house's own model, or Claude through the node's relay, set by the owner per member. Which
//! Claude a role may think with is the plan's models by role (section 9), the same rule the
//! relay enforces on every call: Opus or Sonnet for the CEO; Sonnet or Haiku for the lead, the
//! developer and the reviewer; Haiku only for the researcher. Opus was the CEO's alone until one
//! team (ADR 0142): now any member but a researcher may be given it.
//!
//! Every member has two (ADR 0131): a **work brain**, one of the house's models (the Mac or
//! the NAS's), which does its reading, building and drafting for nothing, and a **judgment
//! brain**, a Claude, which decides. A member who is given none judges with Haiku.

use super::work::{Refusal, Role};

/// What a member thinks with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Brain {
    /// The house's own model, through the node's local relay: nothing leaves the house, and
    /// it costs nothing.
    #[default]
    Local,
    /// The owner's Mac, through the node's local relay (ADR 0130): a stronger model on a laptop
    /// the owner also uses. Nothing leaves the house and it costs nothing, but a turn runs only
    /// while the Mac is plugged in, switched on for the team and idle, one turn at a time, and
    /// waits for it otherwise: it never falls back to another brain.
    Mac,
    /// Claude Sonnet, through the node's Anthropic relay, metered against the team's pool.
    Sonnet,
    /// Claude Haiku, through the same relay.
    Haiku,
    /// Claude Opus, through the same relay: the CEO's only (ADR 0111).
    Opus,
    /// DeepSeek Flash, through the node's local relay to the DeepSeek entry on the owner's
    /// Everyday list (the owner, 2026-10-28): a cloud model, so it is offered nothing
    /// Restricted (ADR 0141), and metered against the team's pool at DeepSeek's prices.
    DeepSeek,
}

/// Every brain, in the order the owner is offered them.
const ALL: [Brain; 6] = [
    Brain::Local,
    Brain::Mac,
    Brain::Opus,
    Brain::Sonnet,
    Brain::Haiku,
    Brain::DeepSeek,
];

impl Brain {
    /// Its stable name, for storage, the snapshot and the runner: `local`, or the model id the
    /// worker names to the relay.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Mac => "mac",
            Self::Sonnet => "claude-sonnet-5-5",
            Self::Haiku => "claude-haiku-5-5",
            Self::Opus => "claude-opus-5-5",
            Self::DeepSeek => "deepseek-flash",
        }
    }

    /// The lowest rank, the house's model's: where a loop's items start unless the owner
    /// says otherwise.
    pub const BOTTOM: u8 = 6;

    /// Its rank on the team (ADR 0142): 1 is the most able. An item that fails its checks climbs
    /// from the house's model to Opus.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Opus => 1,
            Self::Sonnet => 2,
            Self::Haiku => 3,
            Self::DeepSeek => 4,
            Self::Mac => 5,
            Self::Local => Self::BOTTOM,
        }
    }

    /// What the owner reads on the member's card.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Mac => "Mac",
            Self::Sonnet => "Claude Sonnet",
            Self::Haiku => "Claude Haiku",
            Self::Opus => "Claude Opus",
            Self::DeepSeek => "DeepSeek Flash",
        }
    }

    /// The brain with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        // Its stored name, or the short one the owner says ("sonnet", "deepseek").
        if name == "deepseek" {
            return Some(Self::DeepSeek);
        }
        ALL.into_iter().find(|b| {
            b.name() == name
                || b.name()
                    .split('-')
                    .nth(1)
                    .is_some_and(|short| short == name)
        })
    }

    /// Whether what the member reads leaves the house: every brain but the house's own two (the
    /// node's model and the owner's Mac). The node gives a cloud turn no Confidential result
    /// beyond what the owner allowed (ADR 0109).
    #[must_use]
    pub const fn cloud(self) -> bool {
        !self.house()
    }

    /// Whether the brain is a model in the house, whose context is smaller than Claude's and
    /// whose turns cost nothing: the node's own model, or the owner's Mac (ADR 0130).
    #[must_use]
    pub const fn house(self) -> bool {
        matches!(self, Self::Local | Self::Mac)
    }

    /// Its price class (ADR 0113, amended): what a token costs on it, in thousandths of what it
    /// costs on Opus, by the relay's input prices (Opus $4, Sonnet $2, Haiku $0.10 a million;
    /// their output prices stand in the same ratios). Nothing for the house's model, which costs
    /// nothing. A member's claim on the pool is its role's weight times this, so a cheap brain
    /// claims a small part. `endora-infrastructure`'s relay tests hold this to its prices.
    #[must_use]
    pub const fn price_class(self) -> i64 {
        match self {
            Self::Local | Self::Mac => 0,
            Self::Opus => 1_000,
            Self::Sonnet => 500,
            Self::Haiku => 25,
            Self::DeepSeek => 75,
        }
    }

    /// Every brain, in the order the owner is offered them.
    #[must_use]
    pub const fn all() -> [Self; 6] {
        ALL
    }

    /// Whether a member in `role` may think with this.
    #[must_use]
    pub const fn allowed_for(self, role: Role) -> bool {
        match self {
            Self::Local | Self::Mac | Self::DeepSeek => true,
            // Since one team (ADR 0142) Opus is no longer the CEO's alone: whoever the owner
            // gives it, as Sonnet (the owner, 2026-10-30, to seat it in the league).
            Self::Opus => !matches!(role, Role::Researcher),
            Self::Sonnet => !matches!(role, Role::Researcher),
            Self::Haiku => !matches!(role, Role::Ceo),
        }
    }

    /// The brains a member in `role` may be given, in the order the owner is offered them.
    #[must_use]
    pub fn choices(role: Role) -> Vec<Self> {
        ALL.into_iter().filter(|b| b.allowed_for(role)).collect()
    }

    /// The brains a member may do its work with (ADR 0131), in the order the owner is offered
    /// them: the house's models, the Mac first.
    #[must_use]
    pub const fn work_choices() -> [Self; 2] {
        [Self::Mac, Self::Local]
    }

    /// The brains a member in `role` may be given to judge with (ADR 0131): the Claude models
    /// the role may use, in the order the owner is offered them.
    #[must_use]
    pub fn judgment_choices(role: Role) -> Vec<Self> {
        Self::choices(role)
            .into_iter()
            .filter(|b| b.cloud())
            .collect()
    }

    /// The judgment brain a member in `role` has until the owner chooses another (ADR 0131):
    /// Claude Haiku, the owner's cheap Claude; Sonnet for the CEO, who may not think with Haiku.
    #[must_use]
    pub const fn default_judgment(role: Role) -> Self {
        match role {
            Role::Ceo => Self::Sonnet,
            _ => Self::Haiku,
        }
    }

    /// The judgment brain the owner asked for by `name`, for a member in `role`.
    ///
    /// # Errors
    /// [`Refusal::NotABrain`] for a name that is not a Claude the role may use (Opus for any
    /// role but the CEO, the house's models, anything else).
    pub fn for_judgment(role: Role, name: &str) -> Result<Self, Refusal> {
        let brain = Self::for_role(role, name)?;
        if brain.house() {
            return Err(Refusal::NotABrain {
                role,
                asked: name.chars().take(60).collect(),
            });
        }
        Ok(brain)
    }

    /// The work brain the owner asked for by `name`.
    ///
    /// # Errors
    /// [`Refusal::NotABrain`] for a name that is not one of the house's models.
    pub fn for_work(role: Role, name: &str) -> Result<Self, Refusal> {
        Self::from_name(name.trim())
            .filter(|b| b.house())
            .ok_or_else(|| Refusal::NotABrain {
                role,
                asked: name.chars().take(60).collect(),
            })
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
        assert!(!Brain::Local.cloud() && !Brain::Mac.cloud());
        assert!(Brain::Local.house() && Brain::Mac.house() && !Brain::Haiku.house());
        assert!(Brain::Sonnet.cloud() && Brain::Haiku.cloud() && Brain::Opus.cloud());
        for brain in ALL {
            assert_eq!(Brain::from_name(brain.name()), Some(brain));
        }
    }

    #[test]
    fn a_dearer_brain_is_a_larger_price_class_and_the_house_model_none() {
        assert_eq!(Brain::Local.price_class(), 0);
        assert_eq!(Brain::Mac.price_class(), 0);
        assert!(Brain::Opus.price_class() > Brain::Sonnet.price_class());
        assert!(Brain::Sonnet.price_class() > Brain::Haiku.price_class());
        assert!(Brain::Haiku.price_class() > 0);
    }

    #[test]
    fn the_roles_brains_are_the_plans_models_by_role_and_opus_is_anyones_since_one_team() {
        for role in [Role::Lead, Role::Developer, Role::Reviewer] {
            assert_eq!(
                Brain::choices(role),
                [
                    Brain::Local,
                    Brain::Mac,
                    Brain::Opus,
                    Brain::Sonnet,
                    Brain::Haiku,
                    Brain::DeepSeek
                ]
            );
            assert_eq!(Brain::for_role(role, "claude-opus-5-5"), Ok(Brain::Opus));
            assert_eq!(
                Brain::for_role(role, "claude-sonnet-5-5"),
                Ok(Brain::Sonnet)
            );
        }
        assert_eq!(
            Brain::choices(Role::Researcher),
            [Brain::Local, Brain::Mac, Brain::Haiku, Brain::DeepSeek]
        );
        assert_eq!(
            Brain::for_role(Role::Researcher, "claude-haiku-5-5"),
            Ok(Brain::Haiku)
        );
        // The owner's decision of 2026-10-17 (ADR 0111): the CEO thinks with Opus, Sonnet or
        // the house's model, and Opus is nobody else's.
        assert_eq!(
            Brain::choices(Role::Ceo),
            [
                Brain::Local,
                Brain::Mac,
                Brain::Opus,
                Brain::Sonnet,
                Brain::DeepSeek
            ]
        );
        assert_eq!(
            Brain::for_role(Role::Ceo, "claude-opus-5-5"),
            Ok(Brain::Opus)
        );
        for (role, asked) in [
            (Role::Researcher, "claude-sonnet-5-5"),
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
        let said = Brain::for_role(Role::Researcher, "claude-opus-5-5")
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("thinks with") && said.contains("claude-haiku-5-5"),
            "{said}"
        );
    }

    #[test]
    fn deepseek_flash_is_a_cheap_cloud_brain_ranked_between_haiku_and_the_mac() {
        // The owner, 2026-10-28: DeepSeek's API, as long as nothing above Confidential goes to it
        // (a cloud turn is offered nothing Restricted, ADR 0141) and its tokens are cheap.
        let deep = Brain::from_name("deepseek-flash").unwrap();
        assert_eq!(deep, Brain::DeepSeek);
        assert_eq!(
            Brain::from_name("deepseek"),
            Some(Brain::DeepSeek),
            "its short name"
        );
        assert!(deep.cloud() && !deep.house());
        assert_eq!(deep.label(), "DeepSeek Flash");
        assert!(deep.price_class() > 0 && deep.price_class() < Brain::Sonnet.price_class());
        let ranks: Vec<u8> = [
            Brain::Opus,
            Brain::Sonnet,
            Brain::Haiku,
            Brain::DeepSeek,
            Brain::Mac,
            Brain::Local,
        ]
        .iter()
        .map(|b| b.rank())
        .collect();
        assert_eq!(ranks, [1, 2, 3, 4, 5, 6]);
        assert_eq!(Brain::BOTTOM, 6);
        for role in [
            Role::Developer,
            Role::Researcher,
            Role::Lead,
            Role::Reviewer,
        ] {
            assert!(deep.allowed_for(role), "{role:?}");
        }
        assert!(
            Brain::for_work(Role::Developer, "deepseek-flash").is_err(),
            "not the house's"
        );
    }

    #[test]
    fn a_member_judges_with_haiku_unless_the_owner_says_otherwise_and_the_ceo_with_sonnet() {
        for role in [
            Role::Lead,
            Role::Developer,
            Role::Reviewer,
            Role::Researcher,
        ] {
            assert_eq!(Brain::default_judgment(role), Brain::Haiku, "{role:?}");
        }
        // The relay refuses Haiku for the CEO, so its cheapest Claude is Sonnet.
        assert_eq!(Brain::default_judgment(Role::Ceo), Brain::Sonnet);
        // And the default is always one its role may use, and always a Claude.
        for role in [
            Role::Ceo,
            Role::Lead,
            Role::Developer,
            Role::Reviewer,
            Role::Researcher,
        ] {
            let default = Brain::default_judgment(role);
            assert!(default.allowed_for(role) && default.cloud(), "{role:?}");
            assert!(Brain::judgment_choices(role).contains(&default), "{role:?}");
        }
    }

    #[test]
    fn a_judgment_brain_is_a_claude_its_role_may_use_and_a_work_brain_a_house_model() {
        assert_eq!(
            Brain::for_judgment(Role::Lead, "claude-sonnet-5-5"),
            Ok(Brain::Sonnet)
        );
        assert_eq!(
            Brain::for_judgment(Role::Ceo, "claude-opus-5-5"),
            Ok(Brain::Opus)
        );
        for (role, asked) in [
            (Role::Lead, "local"),
            (Role::Lead, "mac"),
            (Role::Lead, "none"),
            (Role::Researcher, "claude-opus-5-5"),
            (Role::Researcher, "claude-sonnet-5-5"),
            (Role::Ceo, "claude-haiku-5-5"),
        ] {
            assert!(
                matches!(
                    Brain::for_judgment(role, asked),
                    Err(Refusal::NotABrain { .. })
                ),
                "{role:?} {asked}"
            );
        }
        assert_eq!(Brain::for_work(Role::Lead, "mac"), Ok(Brain::Mac));
        assert_eq!(Brain::for_work(Role::Lead, " local "), Ok(Brain::Local));
        assert!(Brain::for_work(Role::Lead, "claude-haiku-5-5").is_err());
    }
}
