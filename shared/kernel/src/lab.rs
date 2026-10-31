//! How the home lab is built (ADR 0112): the standing knowledge every member of HQ's team is
//! given, in the CEO's house log (seeded once, when it is empty) and in the worker's home-lab
//! instructions. One text in one place, so the two cannot drift apart. Words only: what the
//! lab actually holds is the `homelab_map` contract's to say. And the one rule of which paths
//! the team may never touch ([`forbidden_path`]).

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

/// Whether a path is one the team may never touch (ADR 0115): the owner's values live in
/// `group_vars/*.yml`, `host_vars/` and every inventory, and anything named for a vault. The
/// node refuses a branch that changes one at submission, and HQ checks it again at review, by
/// this one rule, so it never rests on one place.
#[must_use]
pub fn forbidden_path(path: &str) -> bool {
    let lower = path.trim().trim_start_matches("./").to_ascii_lowercase();
    lower.split('/').any(|p| {
        matches!(
            p,
            "group_vars"
                | "host_vars"
                | "inventories"
                | "inventory"
                | "inventory.yml"
                | "inventory.ini"
        ) || p.contains("vault")
    })
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
