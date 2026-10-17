//! How the home lab is built (ADR 0112): the standing knowledge every member of HQ's team is
//! given, in the CEO's house log (seeded once, when it is empty) and in the worker's home-lab
//! instructions. One text in one place, so the two cannot drift apart. Words only: what the
//! lab actually holds is the `homelab_map` contract's to say.

/// The title of the house log's entry.
pub const LAB_BUILT_TITLE: &str = "How this lab is built";

/// What the team knows about how the home lab is built, a sentence or two each.
pub const LAB_BUILT: [&str; 3] = [
    "The home lab is managed by ansible-nas: each app is a role, and the playbook creates its \
     container. homelab_map shows who owns what and what depends on what.",
    "A container that crashed may be restarted; anything else (settings, memory limits, new or \
     removed containers) is a proposal to change ansible-nas, for the owner to approve and run.",
    "Containers that run in another's network (e.g. behind gluetun) stop when it restarts and \
     come back on their own: check homelab_map and the current state before treating that as \
     a fault.",
];

/// [`LAB_BUILT`] as one short paragraph, as the worker's instructions carry it.
#[must_use]
pub fn lab_built_paragraph() -> String {
    LAB_BUILT.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paragraph_is_the_three_sentences_on_one_line() {
        let said = lab_built_paragraph();
        assert!(said.starts_with("The home lab is managed by ansible-nas"));
        assert!(said.contains("homelab_map"));
        assert!(said.contains("behind gluetun"));
        assert!(!said.contains('\n'));
        assert!(
            LAB_BUILT.iter().all(|s| !s.contains("  ")),
            "no doubled spaces"
        );
    }
}
