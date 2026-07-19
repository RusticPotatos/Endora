//! Identity & Values context.
//!
//! A [`Value`] is a durable theme the person cares about — health, community,
//! craft — that a [`Direction`](crate::targets::Direction) (North Star) serves. It
//! is the *why* above the North Star. Like everything in the domain it is
//! user-owned: the system may ask what a North Star is *for*, but never invents
//! the value itself.

use crate::error::{DomainError, require_non_empty};
use crate::ids::ValueId;

/// A durable theme the person cares about; the "why" a North Star serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    id: ValueId,
    name: String,
}

impl Value {
    /// Creates a value.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `name` is blank.
    pub fn new(id: ValueId, name: &str) -> Result<Self, DomainError> {
        let name = require_non_empty("value.name", name)?;
        Ok(Self { id, name })
    }

    /// The value's identifier.
    #[must_use]
    pub const fn id(&self) -> ValueId {
        self.id
    }

    /// The value's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::Value;
    use crate::error::DomainError;
    use crate::ids::ValueId;

    #[test]
    fn value_requires_a_name() {
        assert_eq!(
            Value::new(ValueId::new(1), "  "),
            Err(DomainError::EmptyField {
                field: "value.name"
            })
        );
    }

    #[test]
    fn value_trims_its_name() {
        let v = Value::new(ValueId::new(1), "  Health  ").unwrap();
        assert_eq!(v.name(), "Health");
    }
}
