//! Use cases for the Direction & Goals slice.
//!
//! These orchestrate the domain and the ports. They are the seam the interfaces
//! (node, CLI) call; they hold no transport or storage detail. Identifiers and
//! time come from the [`IdSource`] and [`Clock`] ports, so the domain stays pure
//! and the use cases stay testable with fakes.

use endora_domain::{Direction, DirectionId, Goal, GoalId};

use crate::error::AppError;
use crate::ports::{DirectionRepository, GoalRepository, IdSource};

/// Creates and stores a new [`Direction`].
///
/// # Errors
/// [`AppError::Domain`] if the title is invalid, or [`AppError::Repository`] if
/// persistence fails.
pub fn create_direction(
    directions: &impl DirectionRepository,
    ids: &impl IdSource,
    title: &str,
) -> Result<Direction, AppError> {
    let direction = Direction::new(DirectionId::new(ids.new_id()), title)?;
    directions.save(&direction)?;
    Ok(direction)
}

/// Creates and stores a new [`Goal`] under an existing direction.
///
/// # Errors
/// [`AppError::NotFound`] if the direction does not exist, [`AppError::Domain`]
/// if the statement is invalid, or [`AppError::Repository`] if persistence fails.
pub fn create_goal(
    directions: &impl DirectionRepository,
    goals: &impl GoalRepository,
    ids: &impl IdSource,
    direction: DirectionId,
    statement: &str,
) -> Result<Goal, AppError> {
    if directions.get(direction)?.is_none() {
        return Err(AppError::NotFound {
            entity: "direction",
        });
    }
    let goal = Goal::new(GoalId::new(ids.new_id()), direction, statement)?;
    goals.save(&goal)?;
    Ok(goal)
}

/// Lists the goals under a direction, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_goals(
    goals: &impl GoalRepository,
    direction: DirectionId,
) -> Result<Vec<Goal>, AppError> {
    Ok(goals.list_for_direction(direction)?)
}

#[cfg(test)]
mod tests {
    use super::{create_direction, create_goal, list_goals};
    use crate::error::AppError;
    use crate::ports::{DirectionRepository, GoalRepository, IdSource, RepositoryError};
    use endora_domain::{Direction, DirectionId, Goal, GoalId};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// An in-memory store implementing both repository ports, for tests only.
    #[derive(Default)]
    struct FakeStore {
        directions: RefCell<HashMap<u128, Direction>>,
        goals: RefCell<HashMap<u128, Goal>>,
    }

    impl DirectionRepository for FakeStore {
        fn save(&self, direction: &Direction) -> Result<(), RepositoryError> {
            self.directions
                .borrow_mut()
                .insert(direction.id().value(), direction.clone());
            Ok(())
        }
        fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError> {
            Ok(self.directions.borrow().get(&id.value()).cloned())
        }
    }

    impl GoalRepository for FakeStore {
        fn save(&self, goal: &Goal) -> Result<(), RepositoryError> {
            self.goals
                .borrow_mut()
                .insert(goal.id().value(), goal.clone());
            Ok(())
        }
        fn get(&self, id: GoalId) -> Result<Option<Goal>, RepositoryError> {
            Ok(self.goals.borrow().get(&id.value()).cloned())
        }
        fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Goal>, RepositoryError> {
            let mut found: Vec<Goal> = self
                .goals
                .borrow()
                .values()
                .filter(|g| g.direction() == direction)
                .cloned()
                .collect();
            found.sort_by_key(|g| g.id().value());
            Ok(found)
        }
    }

    /// A deterministic id source: 1, 2, 3, ...
    #[derive(Default)]
    struct SeqIds {
        next: Cell<u128>,
    }

    impl IdSource for SeqIds {
        fn new_id(&self) -> u128 {
            let id = self.next.get() + 1;
            self.next.set(id);
            id
        }
    }

    #[test]
    fn create_direction_persists_and_returns_it() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let d = create_direction(&store, &ids, "Be healthier").unwrap();
        assert_eq!(DirectionRepository::get(&store, d.id()).unwrap(), Some(d));
    }

    #[test]
    fn create_goal_requires_an_existing_direction() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_goal(&store, &store, &ids, DirectionId::new(999), "Run a 5k").unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "direction"
            }
        );
    }

    #[test]
    fn create_goal_under_a_direction_then_list() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();

        let g1 = create_goal(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        let g2 = create_goal(&store, &store, &ids, direction.id(), "Sleep 8h").unwrap();

        assert_eq!(list_goals(&store, direction.id()).unwrap(), vec![g1, g2]);
    }

    #[test]
    fn invalid_title_is_a_domain_error() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_direction(&store, &ids, "   ").unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }
}
