//! What a member thinks with (the owner's decisions of 2026-10-17, ADR 0109 and 0111): the
//! house's own model, or Claude through the node's relay, set by the owner per member. Which
//! Claude a role may think with is the plan's models by role (section 9), the same rule the
//! relay enforces on every call: Opus or Sonnet for the CEO; Sonnet or Haiku for the lead, the
//! developer and the reviewer; Haiku only for the researcher. Opus was the CEO's alone until one
//! team (ADR 0142): now any member but a researcher may be given it.
//!
//! A member is its model (ADR 0142): the two brains of ADR 0131, one to work and one to judge,
//! are one. A member who is given none is the house's own model.

use super::work::Refusal;

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

    /// Whether the brain is a Claude, which speaks Anthropic's API: Opus, Sonnet or Haiku. Not
    /// every cloud brain is one: DeepSeek is reached through the node's local relay, in
    /// OpenAI's shape, so Claude Code cannot run it (ADR 0117, amended 2026-10-31). A Claude is
    /// also paid from the owner's Anthropic credit, and so from the plan's grant, which sizes
    /// the day's pool (ADR 0113, amended 2026-10-31).
    #[must_use]
    pub const fn claude(self) -> bool {
        matches!(self, Self::Opus | Self::Sonnet | Self::Haiku)
    }

    /// Its price class (ADR 0113, amended): what a token costs on it, in thousandths of what it
    /// costs on Opus, by the relay's input prices (Opus $4, Sonnet $2, Haiku $0.10 a million;
    /// their output prices stand in the same ratios). Nothing for the house's model, which costs
    /// nothing. What a turn's opening call costs is reckoned from it ([`super::money::least_a_turn`]);
    /// a member's claim on the pool is not, since 2026-11-01 ([`Self::claim_class`]).
    /// `endora-infrastructure`'s relay tests hold this to its prices.
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

    /// Its claim class (ADR 0113, amended 2026-11-01): what a member's claim on the day's pool is
    /// its role's weight times. The owner, 1 Nov: most of the money goes to Haiku, the cheapest
    /// paid model and the one doing most of the work; the climbs to Sonnet and Opus draw on what
    /// is left. So Haiku claims four times what Sonnet or DeepSeek does, and Opus, which only an
    /// item that failed twice below reaches, half of that. Round 3 weighted claims by price, and
    /// Ada on Haiku had $0.19 of a $16.60 day. Nothing for the house's model, which costs nothing.
    #[must_use]
    pub const fn claim_class(self) -> i64 {
        match self {
            Self::Local | Self::Mac => 0,
            Self::Opus => 1,
            Self::Sonnet | Self::DeepSeek => 2,
            Self::Haiku => 8,
        }
    }

    /// Every brain, in the order the owner is offered them.
    #[must_use]
    pub const fn all() -> [Self; 6] {
        ALL
    }

    /// The brain the owner asked for by `name`: any member may be any model (ADR 0142).
    ///
    /// # Errors
    /// [`Refusal::NotABrain`] for a name that is not one.
    pub fn named(name: &str) -> Result<Self, Refusal> {
        Self::from_name(name.trim()).ok_or_else(|| Refusal::NotABrain {
            asked: name.chars().take(60).collect(),
        })
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
    fn any_member_may_be_any_model_endora_has_and_nothing_else() {
        // A title is a name only (ADR 0142): the owner may give anyone any of the models.
        for brain in ALL {
            assert_eq!(Brain::named(brain.name()), Ok(brain));
        }
        assert_eq!(Brain::named(" claude-opus-5-5 "), Ok(Brain::Opus));
        for asked in ["gpt-5", "", "claude-sonnet-5-5-20261001"] {
            assert!(
                matches!(Brain::named(asked), Err(Refusal::NotABrain { .. })),
                "{asked}"
            );
        }
        let said = Brain::named("gpt-5").unwrap_err().to_string();
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
    }
}
