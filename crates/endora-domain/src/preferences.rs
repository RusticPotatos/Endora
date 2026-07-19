//! Preferences — what the butler has learned about the person.
//!
//! A [`Preference`] is a durable thing the person wants the butler to keep in
//! mind ("prefers mornings", "be terse", "running is about community"). This is
//! how the butler "gets smarter over time": learning is the accumulation of
//! preferences, not opaque model training (see
//! `docs/adr/0010-autonomy-model.md`). Preferences are user-owned memory —
//! visible, correctable, and deletable — and the butler feeds them back into its
//! own context so it stops asking the same thing twice.
//!
//! ADR 0010 draws two kinds. **Taste** (style/defaults) *may* be inferred;
//! **grants of authority** may *only* be explicitly stated. In this build every
//! preference is created by explicit confirmation, so both kinds are effectively
//! "stated"; the inference path (which would enforce "never infer authority")
//! is future work and is why the kind is recorded now.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{PreferenceId, Timestamp};

/// The two kinds of preference from the autonomy model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceKind {
    /// Taste/style — a default within already-permitted, reversible space. May
    /// (in future) be inferred; a wrong guess is cheap and correctable.
    Taste,
    /// A grant of authority — expands what the butler may do on its own. Only
    /// ever explicitly stated by the person, never inferred.
    Authority,
}

impl PreferenceKind {
    /// A stable, lowercase name for storage and the protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Taste => "taste",
            Self::Authority => "authority",
        }
    }

    /// Parses a kind from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "taste" => Some(Self::Taste),
            "authority" => Some(Self::Authority),
            _ => None,
        }
    }
}

/// A durable preference the butler keeps in mind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preference {
    id: PreferenceId,
    text: String,
    kind: PreferenceKind,
    at: Timestamp,
}

impl Preference {
    /// Records a preference at a caller-supplied time.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `text` is blank.
    pub fn new(
        id: PreferenceId,
        text: &str,
        kind: PreferenceKind,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        let text = require_non_empty("preference.text", text)?;
        Ok(Self { id, text, kind, at })
    }

    /// The preference's identifier.
    #[must_use]
    pub const fn id(&self) -> PreferenceId {
        self.id
    }

    /// The preference text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The kind (taste or authority).
    #[must_use]
    pub const fn kind(&self) -> PreferenceKind {
        self.kind
    }

    /// When it was recorded.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }
}

#[cfg(test)]
mod tests {
    use super::{Preference, PreferenceKind};
    use crate::error::DomainError;
    use crate::ids::{PreferenceId, Timestamp};

    #[test]
    fn kind_names_round_trip() {
        for k in [PreferenceKind::Taste, PreferenceKind::Authority] {
            assert_eq!(PreferenceKind::from_name(k.name()), Some(k));
        }
        assert_eq!(PreferenceKind::from_name("bogus"), None);
    }

    #[test]
    fn preference_keeps_its_fields() {
        let at = Timestamp::from_unix_millis(1_000);
        let p = Preference::new(
            PreferenceId::new(1),
            "  prefers mornings  ",
            PreferenceKind::Taste,
            at,
        )
        .unwrap();
        assert_eq!(p.text(), "prefers mornings");
        assert_eq!(p.kind(), PreferenceKind::Taste);
        assert_eq!(p.at(), at);
    }

    #[test]
    fn preference_rejects_blank_text() {
        assert_eq!(
            Preference::new(
                PreferenceId::new(1),
                "   ",
                PreferenceKind::Authority,
                Timestamp::from_unix_millis(0)
            ),
            Err(DomainError::EmptyField {
                field: "preference.text"
            })
        );
    }
}
