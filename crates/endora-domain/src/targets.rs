//! Direction & Targets context.
//!
//! The user's stated [`Direction`] (their North Star context), the [`Target`]s
//! pursued under it, and the [`Assumption`]s a target rests on. These are
//! user-owned: the domain validates and holds them but never invents them.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{AssumptionId, DirectionId, TargetId};

/// The lifecycle state of a [`Direction`] or [`Target`].
///
/// Transitions are deliberately flexible and reversible: any state can move to
/// any other via [`set_status`](Direction::set_status), because a person may
/// reopen something they achieved, abandoned, or archived. Archiving simply puts
/// an item away out of the active view without deleting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleStatus {
    /// Being actively pursued (the default for a new item).
    Active,
    /// Reached — a success outcome.
    Achieved,
    /// Deliberately let go without being reached.
    Abandoned,
    /// Put away out of the active view, kept for the record.
    Archived,
}

impl LifecycleStatus {
    /// A stable, lowercase name for storage and the protocol. Round-trips with
    /// [`from_name`](Self::from_name).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Achieved => "achieved",
            Self::Abandoned => "abandoned",
            Self::Archived => "archived",
        }
    }

    /// Parses a status from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "active" => Some(Self::Active),
            "achieved" => Some(Self::Achieved),
            "abandoned" => Some(Self::Abandoned),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    /// Whether this is the default, in-progress state.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// The user's stated direction — the North Star context a target serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Direction {
    id: DirectionId,
    title: String,
    status: LifecycleStatus,
}

impl Direction {
    /// Creates an [`Active`](LifecycleStatus::Active) direction.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `title` is blank.
    pub fn new(id: DirectionId, title: &str) -> Result<Self, DomainError> {
        let title = require_non_empty("direction.title", title)?;
        Ok(Self {
            id,
            title,
            status: LifecycleStatus::Active,
        })
    }

    /// Reconstitutes a direction from persisted parts, including its stored
    /// lifecycle status. For storage adapters loading a saved direction.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `title` is blank.
    pub fn from_parts(
        id: DirectionId,
        title: &str,
        status: LifecycleStatus,
    ) -> Result<Self, DomainError> {
        let title = require_non_empty("direction.title", title)?;
        Ok(Self { id, title, status })
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

    /// The direction's lifecycle status.
    #[must_use]
    pub const fn status(&self) -> LifecycleStatus {
        self.status
    }

    /// Sets the lifecycle status (achieve, abandon, archive, or reopen).
    pub const fn set_status(&mut self, status: LifecycleStatus) {
        self.status = status;
    }
}

/// A single intentional objective pursued under a [`Direction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    id: TargetId,
    direction: DirectionId,
    statement: String,
    status: LifecycleStatus,
}

impl Target {
    /// Creates an [`Active`](LifecycleStatus::Active) target under a direction.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank.
    pub fn new(id: TargetId, direction: DirectionId, statement: &str) -> Result<Self, DomainError> {
        let statement = require_non_empty("target.statement", statement)?;
        Ok(Self {
            id,
            direction,
            statement,
            status: LifecycleStatus::Active,
        })
    }

    /// Reconstitutes a target from persisted parts, including its stored
    /// lifecycle status. For storage adapters loading a saved target.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank.
    pub fn from_parts(
        id: TargetId,
        direction: DirectionId,
        statement: &str,
        status: LifecycleStatus,
    ) -> Result<Self, DomainError> {
        let statement = require_non_empty("target.statement", statement)?;
        Ok(Self {
            id,
            direction,
            statement,
            status,
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

    /// The target's lifecycle status.
    #[must_use]
    pub const fn status(&self) -> LifecycleStatus {
        self.status
    }

    /// Sets the lifecycle status (achieve, abandon, archive, or reopen).
    pub const fn set_status(&mut self, status: LifecycleStatus) {
        self.status = status;
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
    use super::{Assumption, Direction, LifecycleStatus, Target};
    use crate::error::DomainError;
    use crate::ids::{AssumptionId, DirectionId, TargetId};

    #[test]
    fn a_new_direction_and_target_start_active() {
        let d = Direction::new(DirectionId::new(1), "Be healthier").unwrap();
        assert_eq!(d.status(), LifecycleStatus::Active);
        let t = Target::new(TargetId::new(2), DirectionId::new(1), "Run a 5k").unwrap();
        assert_eq!(t.status(), LifecycleStatus::Active);
    }

    #[test]
    fn lifecycle_status_is_settable_and_reversible() {
        let mut t = Target::new(TargetId::new(2), DirectionId::new(1), "Run a 5k").unwrap();
        t.set_status(LifecycleStatus::Achieved);
        assert_eq!(t.status(), LifecycleStatus::Achieved);
        // Reopening an achieved target is allowed.
        t.set_status(LifecycleStatus::Active);
        assert_eq!(t.status(), LifecycleStatus::Active);
    }

    #[test]
    fn lifecycle_status_names_round_trip() {
        for s in [
            LifecycleStatus::Active,
            LifecycleStatus::Achieved,
            LifecycleStatus::Abandoned,
            LifecycleStatus::Archived,
        ] {
            assert_eq!(LifecycleStatus::from_name(s.name()), Some(s));
        }
        assert_eq!(LifecycleStatus::from_name("bogus"), None);
        assert!(LifecycleStatus::Active.is_active());
        assert!(!LifecycleStatus::Archived.is_active());
    }

    #[test]
    fn from_parts_restores_a_stored_status() {
        let d = Direction::from_parts(
            DirectionId::new(1),
            "Be healthier",
            LifecycleStatus::Archived,
        )
        .unwrap();
        assert_eq!(d.status(), LifecycleStatus::Archived);
        let t = Target::from_parts(
            TargetId::new(2),
            DirectionId::new(1),
            "Run a 5k",
            LifecycleStatus::Abandoned,
        )
        .unwrap();
        assert_eq!(t.status(), LifecycleStatus::Abandoned);
    }

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
