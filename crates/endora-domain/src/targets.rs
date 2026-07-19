//! Direction & Targets context.
//!
//! The user's stated [`Direction`] (their North Star context), the [`Target`]s
//! pursued under it, and the [`Assumption`]s a target rests on. These are
//! user-owned: the domain validates and holds them but never invents them.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{AssumptionId, DirectionId, TargetId};

/// The user's stated direction — the North Star context a target serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Direction {
    id: DirectionId,
    title: String,
}

impl Direction {
    /// Creates a direction.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `title` is blank.
    pub fn new(id: DirectionId, title: &str) -> Result<Self, DomainError> {
        let title = require_non_empty("direction.title", title)?;
        Ok(Self { id, title })
    }

    /// The direction's identifier.
    #[must_use]
    pub const fn id(&self) -> DirectionId {
        self.id
    }

    /// The direction's title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
}

/// A single intentional objective pursued under a [`Direction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    id: TargetId,
    direction: DirectionId,
    statement: String,
}

impl Target {
    /// Creates a target under a direction.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank.
    pub fn new(id: TargetId, direction: DirectionId, statement: &str) -> Result<Self, DomainError> {
        let statement = require_non_empty("target.statement", statement)?;
        Ok(Self {
            id,
            direction,
            statement,
        })
    }

    /// The target's identifier.
    #[must_use]
    pub const fn id(&self) -> TargetId {
        self.id
    }

    /// The direction this target serves.
    #[must_use]
    pub const fn direction(&self) -> DirectionId {
        self.direction
    }

    /// The target statement.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
}

/// A belief a [`Target`] rests on, made explicit so it can be tested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumption {
    id: AssumptionId,
    target: TargetId,
    statement: String,
}

impl Assumption {
    /// Creates an assumption tied to a target.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank.
    pub fn new(id: AssumptionId, target: TargetId, statement: &str) -> Result<Self, DomainError> {
        let statement = require_non_empty("assumption.statement", statement)?;
        Ok(Self {
            id,
            target,
            statement,
        })
    }

    /// The assumption's identifier.
    #[must_use]
    pub const fn id(&self) -> AssumptionId {
        self.id
    }

    /// The target this assumption belongs to.
    #[must_use]
    pub const fn target(&self) -> TargetId {
        self.target
    }

    /// The assumption statement.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
}

#[cfg(test)]
mod tests {
    use super::{Assumption, Direction, Target};
    use crate::error::DomainError;
    use crate::ids::{AssumptionId, DirectionId, TargetId};

    #[test]
    fn direction_requires_a_title() {
        assert_eq!(
            Direction::new(DirectionId::new(1), "  "),
            Err(DomainError::EmptyField {
                field: "direction.title"
            })
        );
    }

    #[test]
    fn direction_trims_its_title() {
        let d = Direction::new(DirectionId::new(1), "  Be healthier  ").unwrap();
        assert_eq!(d.title(), "Be healthier");
    }

    #[test]
    fn target_links_to_its_direction() {
        let target = Target::new(TargetId::new(2), DirectionId::new(1), "Run a 5k").unwrap();
        assert_eq!(target.direction(), DirectionId::new(1));
        assert_eq!(target.statement(), "Run a 5k");
    }

    #[test]
    fn target_rejects_empty_statement() {
        assert_eq!(
            Target::new(TargetId::new(2), DirectionId::new(1), ""),
            Err(DomainError::EmptyField {
                field: "target.statement"
            })
        );
    }

    #[test]
    fn assumption_links_to_its_target() {
        let a = Assumption::new(
            AssumptionId::new(3),
            TargetId::new(2),
            "Mornings are freest",
        )
        .unwrap();
        assert_eq!(a.target(), TargetId::new(2));
    }
}
