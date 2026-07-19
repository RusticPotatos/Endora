//! Direction & Goals context.
//!
//! The user's stated [`Direction`] (their North Star context), the [`Goal`]s
//! pursued under it, and the [`Assumption`]s a goal rests on. These are
//! user-owned: the domain validates and holds them but never invents them.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{AssumptionId, DirectionId, GoalId};

/// The user's stated direction — the North Star context a goal serves.
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
pub struct Goal {
    id: GoalId,
    direction: DirectionId,
    statement: String,
}

impl Goal {
    /// Creates a goal under a direction.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank.
    pub fn new(id: GoalId, direction: DirectionId, statement: &str) -> Result<Self, DomainError> {
        let statement = require_non_empty("goal.statement", statement)?;
        Ok(Self {
            id,
            direction,
            statement,
        })
    }

    /// The goal's identifier.
    #[must_use]
    pub const fn id(&self) -> GoalId {
        self.id
    }

    /// The direction this goal serves.
    #[must_use]
    pub const fn direction(&self) -> DirectionId {
        self.direction
    }

    /// The goal statement.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
}

/// A belief a [`Goal`] rests on, made explicit so it can be tested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumption {
    id: AssumptionId,
    goal: GoalId,
    statement: String,
}

impl Assumption {
    /// Creates an assumption tied to a goal.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank.
    pub fn new(id: AssumptionId, goal: GoalId, statement: &str) -> Result<Self, DomainError> {
        let statement = require_non_empty("assumption.statement", statement)?;
        Ok(Self {
            id,
            goal,
            statement,
        })
    }

    /// The assumption's identifier.
    #[must_use]
    pub const fn id(&self) -> AssumptionId {
        self.id
    }

    /// The goal this assumption belongs to.
    #[must_use]
    pub const fn goal(&self) -> GoalId {
        self.goal
    }

    /// The assumption statement.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
}

#[cfg(test)]
mod tests {
    use super::{Assumption, Direction, Goal};
    use crate::error::DomainError;
    use crate::ids::{AssumptionId, DirectionId, GoalId};

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
    fn goal_links_to_its_direction() {
        let goal = Goal::new(GoalId::new(2), DirectionId::new(1), "Run a 5k").unwrap();
        assert_eq!(goal.direction(), DirectionId::new(1));
        assert_eq!(goal.statement(), "Run a 5k");
    }

    #[test]
    fn goal_rejects_empty_statement() {
        assert_eq!(
            Goal::new(GoalId::new(2), DirectionId::new(1), ""),
            Err(DomainError::EmptyField {
                field: "goal.statement"
            })
        );
    }

    #[test]
    fn assumption_links_to_its_goal() {
        let a =
            Assumption::new(AssumptionId::new(3), GoalId::new(2), "Mornings are freest").unwrap();
        assert_eq!(a.goal(), GoalId::new(2));
    }
}
