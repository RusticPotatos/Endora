//! How sensitive a piece of data is (ADR 0105). Code sets it by where the data came from,
//! never a model's judgement; a request's tier is the highest of anything in it; a model's
//! clearance decides what it may see. A local model is cleared for every tier; a cloud model
//! sees Confidential only disguised, and Restricted never.

/// A data tier, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Anyone may know it: a web page, the news, the weather.
    Public,
    /// The house's own workings: container and service status, Endora's settings.
    Internal,
    /// The owner's life: their messages, calendar, what Endora knows about them, where they
    /// are. A cloud model sees it only disguised.
    Confidential,
    /// What must not leave the house in any form: what the owner marks 🔒, account numbers,
    /// a movement history. Only a local model sees it.
    Restricted,
}

impl Tier {
    /// As the owner reads it.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Public => "Public",
            Self::Internal => "Internal",
            Self::Confidential => "Confidential",
            Self::Restricted => "Restricted",
        }
    }

    /// Whether a model off the device may see data of this tier at all (disguised, for
    /// Confidential).
    #[must_use]
    pub const fn may_leave(self) -> bool {
        !matches!(self, Self::Restricted)
    }
}

#[cfg(test)]
mod tests {
    use super::Tier;

    #[test]
    fn tiers_order_lowest_first_and_only_restricted_stays() {
        assert!(Tier::Public < Tier::Internal && Tier::Internal < Tier::Confidential);
        assert!(Tier::Confidential < Tier::Restricted);
        assert_eq!(
            [Tier::Internal, Tier::Restricted, Tier::Public]
                .into_iter()
                .max(),
            Some(Tier::Restricted)
        );
        assert!(Tier::Confidential.may_leave() && !Tier::Restricted.may_leave());
    }
}
