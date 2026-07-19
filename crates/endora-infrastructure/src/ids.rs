//! A random identifier source.

use endora_application::IdSource;
use uuid::Uuid;

/// An [`IdSource`] backed by random (v4) UUIDs mapped to `u128`.
///
/// Random 128-bit ids are effectively unique across restarts without any
/// coordination, which suits a local-first node.
#[derive(Debug, Default, Clone, Copy)]
pub struct RandomIdSource;

impl IdSource for RandomIdSource {
    fn new_id(&self) -> u128 {
        Uuid::new_v4().as_u128()
    }
}

#[cfg(test)]
mod tests {
    use super::RandomIdSource;
    use endora_application::IdSource;

    #[test]
    fn produces_distinct_nonzero_ids() {
        let source = RandomIdSource;
        let a = source.new_id();
        let b = source.new_id();
        assert_ne!(a, b);
        assert_ne!(a, 0);
    }
}
